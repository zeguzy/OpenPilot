//! Google Gemini API provider with SSE streaming.
//!
//! Implements [`Provider`] against the Gemini
//! `:streamGenerateContent?alt=sse` endpoint, mapping streamed response
//! chunks to [`ProviderEvent`]s.

use std::future::Future;
use std::pin::Pin;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{Value, json};

use super::message::{Message, MessageRole};
use super::provider::{Provider, ProviderEvent, ProviderStream, StopReason};
use super::tool::ToolDef;

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// LLM provider backed by the Google Gemini API.
pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiProvider {
    #[must_use]
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/{model}:streamGenerateContent?alt=sse",
            GEMINI_BASE_URL,
            model = self.model
        )
    }
}

fn build_contents(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .map(content_entry)
        .collect()
}

fn content_entry(m: &Message) -> Value {
    match m.role {
        MessageRole::User => json!({
            "role": "user",
            "parts": [{ "text": m.text() }],
        }),
        MessageRole::Assistant => {
            let mut parts: Vec<Value> = Vec::new();
            if !m.text().is_empty() {
                parts.push(json!({ "text": m.text() }));
            }
            for tc in m.tool_calls() {
                parts.push(json!({
                    "functionCall": {
                        "name": tc.name,
                        "args": tc.arguments,
                    }
                }));
            }
            json!({ "role": "model", "parts": parts })
        }
        MessageRole::Tool => {
            let name = m.tool_result_info().map_or("", |(id, _)| id);
            let content = m
                .tool_result_info()
                .map_or_else(|| m.text(), |(_, r)| r.content.as_str());
            json!({
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": name,
                        "response": { "name": name, "content": { "result": content } },
                    }
                }]
            })
        }
        MessageRole::System => {
            unreachable!("system messages are filtered before conversion")
        }
    }
}

fn build_tools(tools: &[ToolDef]) -> Vec<Value> {
    if tools.is_empty() {
        return Vec::new();
    }
    let decls: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            })
        })
        .collect();
    vec![json!({ "functionDeclarations": decls })]
}

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "STOP" => StopReason::EndTurn,
        "MAX_TOKENS" => StopReason::MaxTokens,
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" => StopReason::EndTurn,
        _ => StopReason::EndTurn,
    }
}

impl Provider for GeminiProvider {
    #[allow(clippy::too_many_lines)]
    fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send>> {
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let endpoint = self.endpoint();
        let messages: Vec<Message> = messages.to_vec();
        let tools: Vec<ToolDef> = tools.to_vec();

        let mut system_parts: Vec<String> = Vec::new();
        if let Some(sys) = system_prompt {
            system_parts.push(sys.to_owned());
        }
        for m in &messages {
            if m.role == MessageRole::System && !m.text().is_empty() {
                system_parts.push(m.text().to_string());
            }
        }

        Box::pin(async move {
            let mut body = json!({
                "contents": build_contents(&messages),
            });
            if !system_parts.is_empty() {
                body["systemInstruction"] = json!({
                    "parts": [{ "text": system_parts.join("\n\n") }]
                });
            }
            let api_tools = build_tools(&tools);
            if !api_tools.is_empty() {
                body["tools"] = json!(api_tools);
            }

            let response = client
                .post(&endpoint)
                .header("x-goog-api-key", &api_key)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Gemini API error {status}: {text}");
            }

            let (tx, rx) = tokio::sync::mpsc::channel::<anyhow::Result<ProviderEvent>>(64);
            tokio::spawn(async move {
                let mut sse = response.bytes_stream().eventsource();
                let mut call_counter: u64 = 0;

                while let Some(result) = sse.next().await {
                    match result {
                        Ok(event) => {
                            let data: Value = match serde_json::from_str(&event.data) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };

                            if let Some(err_msg) =
                                data.pointer("/error/message").and_then(|v| v.as_str())
                            {
                                let _ =
                                    tx.send(Ok(ProviderEvent::Error(err_msg.to_string()))).await;
                                return;
                            }

                            let Some(candidate) = data.pointer("/candidates/0") else {
                                continue;
                            };

                            let finish_reason = candidate
                                .get("finishReason")
                                .and_then(Value::as_str)
                                .map(map_finish_reason);

                            // Process parts — text deltas and function calls.
                            if let Some(parts) = candidate
                                .pointer("/content/parts")
                                .and_then(|v| v.as_array())
                            {
                                for part in parts {
                                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                        if !text.is_empty()
                                            && tx
                                                .send(Ok(ProviderEvent::TextDelta(
                                                    text.to_string(),
                                                )))
                                                .await
                                                .is_err()
                                        {
                                            return;
                                        }
                                    }

                                    if let Some(fc) = part.get("functionCall") {
                                        let name = fc
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or("unknown");
                                        let args_val =
                                            fc.get("args").cloned().unwrap_or_else(|| json!({}));
                                        let id = format!("gemini_call_{call_counter}");
                                        call_counter += 1;

                                        if tx
                                            .send(Ok(ProviderEvent::ToolCallStart {
                                                id: id.clone(),
                                                name: name.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                        if tx
                                            .send(Ok(ProviderEvent::ToolCallArgs {
                                                id: id.clone(),
                                                args: args_val.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                        if tx
                                            .send(Ok(ProviderEvent::ToolCallEnd { id }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }

                            if let Some(stop) = finish_reason {
                                let _ =
                                    tx.send(Ok(ProviderEvent::Done { stop_reason: stop })).await;
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(anyhow::anyhow!("SSE stream error: {e}"))).await;
                            return;
                        }
                    }
                }
                // Stream ended without an explicit finishReason.
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
