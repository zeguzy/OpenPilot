//! `OpenAI` Chat Completions API provider with SSE streaming.
//!
//! Implements [`Provider`] against the `OpenAI` `/v1/chat/completions` endpoint
//! (`stream: true`), mapping streamed delta chunks to [`ProviderEvent`]s.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{Value, json};

use super::message::{Message, MessageRole};
use super::provider::{Provider, ProviderEvent, ProviderStream, StopReason};
use super::tool::ToolDef;

/// LLM provider backed by the `OpenAI` Chat Completions API.
///
/// Works with any OpenAI-compatible endpoint (`DeepSeek`, Zhipu, Ollama,
/// …) via [`OpenAIProvider::with_base_url`].
pub struct OpenAIProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    /// Full path to the chat completions endpoint (including
    /// `/chat/completions`).
    base_url: String,
}

impl OpenAIProvider {
    /// Create a provider targeting the canonical `OpenAI` endpoint.
    #[must_use]
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_base_url(api_key, model, "https://api.openai.com/v1/chat/completions")
    }

    /// Create a provider targeting a custom OpenAI-compatible endpoint.
    ///
    /// `base_url` must be the **full** path to the chat completions
    /// endpoint, e.g. `https://open.bigmodel.cn/api/paas/v4/chat/completions`.
    /// Use [`crate::provider::presets::normalize_chat_completions_url`] to
    /// turn a bare base into a full endpoint URL.
    #[must_use]
    pub fn with_base_url(api_key: &str, model: &str, base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: base_url.to_string(),
        }
    }
}

fn build_api_messages(messages: &[Message], system_prompt: Option<&str>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();

    if let Some(sys) = system_prompt {
        out.push(json!({ "role": "system", "content": sys }));
    }

    for m in messages {
        match m.role {
            MessageRole::System => {
                out.push(json!({ "role": "system", "content": m.content }));
            }
            MessageRole::User => {
                out.push(json!({ "role": "user", "content": m.content }));
            }
            MessageRole::Assistant => {
                let mut msg = json!({ "role": "assistant", "content": m.content });
                if !m.tool_calls.is_empty() {
                    let tool_calls: Vec<Value> = m
                        .tool_calls
                        .iter()
                        .map(|tc| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    msg["tool_calls"] = json!(tool_calls);
                }
                out.push(msg);
            }
            MessageRole::Tool => {
                let tool_call_id = m.tool_call_id.as_deref().unwrap_or("");
                let content = m
                    .tool_result
                    .as_ref()
                    .map_or(m.content.as_str(), |r| r.content.as_str());
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content,
                }));
            }
        }
    }
    out
}

fn build_api_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::EndTurn,
        _ => StopReason::EndTurn,
    }
}

/// Emit `ToolCallEnd` for every tracked tool-call index that is still open.
async fn close_tool_calls(
    tx: &tokio::sync::mpsc::Sender<anyhow::Result<ProviderEvent>>,
    index_to_id: &HashMap<u64, String>,
    open_indices: &[u64],
) {
    for idx in open_indices {
        if let Some(id) = index_to_id.get(idx) {
            let _ = tx
                .send(Ok(ProviderEvent::ToolCallEnd { id: id.clone() }))
                .await;
        }
    }
}

impl Provider for OpenAIProvider {
    #[allow(clippy::too_many_lines)]
    fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send>> {
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let base_url = self.base_url.clone();
        let messages: Vec<Message> = messages.to_vec();
        let tools: Vec<ToolDef> = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_owned);

        Box::pin(async move {
            let api_messages = build_api_messages(&messages, system_prompt.as_deref());

            let mut body = json!({
                "model": model,
                "stream": true,
                "messages": api_messages,
            });
            if !tools.is_empty() {
                body["tools"] = json!(build_api_tools(&tools));
            }

            let response = client
                .post(&base_url)
                .bearer_auth(&api_key)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("OpenAI API error {status}: {text}");
            }

            let (tx, rx) = tokio::sync::mpsc::channel::<anyhow::Result<ProviderEvent>>(64);
            tokio::spawn(async move {
                let mut sse = response.bytes_stream().eventsource();

                // State for correlating streamed tool-call deltas by index.
                let mut index_to_id: HashMap<u64, String> = HashMap::new();
                let mut current_index: Option<u64> = None;
                let mut open_indices: Vec<u64> = Vec::new();

                while let Some(result) = sse.next().await {
                    match result {
                        Ok(event) => {
                            // OpenAI signals end-of-stream with `data: [DONE]`.
                            if event.data.trim() == "[DONE]" {
                                close_tool_calls(&tx, &index_to_id, &open_indices).await;
                                let _ = tx
                                    .send(Ok(ProviderEvent::Done {
                                        stop_reason: StopReason::EndTurn,
                                    }))
                                    .await;
                                return;
                            }

                            let data: Value = match serde_json::from_str(&event.data) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };

                            // API-level error.
                            if let Some(err_msg) =
                                data.pointer("/error/message").and_then(|v| v.as_str())
                            {
                                let _ =
                                    tx.send(Ok(ProviderEvent::Error(err_msg.to_string()))).await;
                                return;
                            }

                            let Some(choice) = data.pointer("/choices/0") else {
                                continue;
                            };

                            if let Some(delta) = choice.get("delta") {
                                // Text delta.
                                if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                                    if !text.is_empty()
                                        && tx
                                            .send(Ok(ProviderEvent::TextDelta(text.to_string())))
                                            .await
                                            .is_err()
                                    {
                                        return;
                                    }
                                }

                                // Tool-call deltas.
                                if let Some(tool_calls) =
                                    delta.get("tool_calls").and_then(|v| v.as_array())
                                {
                                    for tc in tool_calls {
                                        let index =
                                            tc.get("index").and_then(Value::as_u64).unwrap_or(0);

                                        // Switching to a new index → close the previous.
                                        if let Some(prev) = current_index {
                                            if prev != index && open_indices.contains(&prev) {
                                                if let Some(prev_id) = index_to_id.get(&prev) {
                                                    if tx
                                                        .send(Ok(ProviderEvent::ToolCallEnd {
                                                            id: prev_id.clone(),
                                                        }))
                                                        .await
                                                        .is_err()
                                                    {
                                                        return;
                                                    }
                                                }
                                                open_indices.retain(|&i| i != prev);
                                            }
                                        }

                                        let id_opt = tc.get("id").and_then(|v| v.as_str());
                                        let name_opt =
                                            tc.pointer("/function/name").and_then(|v| v.as_str());
                                        let args_delta = tc
                                            .pointer("/function/arguments")
                                            .and_then(|v| v.as_str());

                                        // New tool call: both id and name present.
                                        if let (Some(call_id), Some(call_name)) = (id_opt, name_opt)
                                        {
                                            index_to_id.insert(index, call_id.to_string());
                                            if !open_indices.contains(&index) {
                                                open_indices.push(index);
                                            }
                                            current_index = Some(index);
                                            if tx
                                                .send(Ok(ProviderEvent::ToolCallStart {
                                                    id: call_id.to_string(),
                                                    name: call_name.to_string(),
                                                }))
                                                .await
                                                .is_err()
                                            {
                                                return;
                                            }
                                        }

                                        // Arguments delta — look up real id from state.
                                        if let Some(args) = args_delta {
                                            if !args.is_empty() {
                                                let id = index_to_id
                                                    .get(&index)
                                                    .cloned()
                                                    .unwrap_or_else(|| format!("call_idx_{index}"));
                                                if tx
                                                    .send(Ok(ProviderEvent::ToolCallArgs {
                                                        id,
                                                        args: args.to_string(),
                                                    }))
                                                    .await
                                                    .is_err()
                                                {
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // finish_reason present → done.
                            if let Some(finish) =
                                choice.get("finish_reason").and_then(|v| v.as_str())
                            {
                                if finish != "null" {
                                    let stop = map_finish_reason(finish);
                                    close_tool_calls(&tx, &index_to_id, &open_indices).await;
                                    let _ = tx
                                        .send(Ok(ProviderEvent::Done { stop_reason: stop }))
                                        .await;
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(anyhow::anyhow!("SSE stream error: {e}"))).await;
                            return;
                        }
                    }
                }
                // Stream ended without [DONE] or finish_reason.
                close_tool_calls(&tx, &index_to_id, &open_indices).await;
                let _ = tx
                    .send(Ok(ProviderEvent::Done {
                        stop_reason: StopReason::EndTurn,
                    }))
                    .await;
            });

            let stream: ProviderStream = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
            Ok(stream)
        })
    }
}
