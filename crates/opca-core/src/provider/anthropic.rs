//! Anthropic Messages API provider with SSE streaming.
//!
//! Implements [`Provider`] against the Anthropic `/v1/messages` endpoint,
//! mapping server-sent events to [`ProviderEvent`]s.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{Value, json};

use super::message::{Message, MessagePart, MessageRole};
use super::provider::{Provider, ProviderEvent, ProviderStream, StopReason};
use super::tool::ToolDef;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u64 = 4096;

/// LLM provider backed by the Anthropic Messages API.
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    #[must_use]
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

/// Convert internal [`Message`] list to Anthropic messages JSON array.
///
/// System-role messages are excluded here — they are sent in the top-level
/// `system` field instead.
fn build_api_messages(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .map(api_message)
        .collect()
}

fn api_message(m: &Message) -> Value {
    match m.role {
        MessageRole::Tool | MessageRole::User => {
            if let Some((tool_use_id, result)) = m.tool_result_info() {
                return json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": result.content,
                        "is_error": result.is_error,
                    }]
                });
            }
            let blocks = text_and_thinking_blocks(m);
            json!({ "role": "user", "content": blocks })
        }
        MessageRole::Assistant => {
            let mut blocks: Vec<Value> = Vec::new();
            for part in &m.parts {
                match part {
                    MessagePart::Text(t) if !t.is_empty() => {
                        blocks.push(json!({ "type": "text", "text": t }));
                    }
                    MessagePart::Thinking(t) => {
                        blocks.push(json!({ "type": "thinking", "thinking": t }));
                    }
                    MessagePart::ToolCall(tc) => {
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments,
                        }));
                    }
                    _ => {}
                }
            }
            json!({ "role": "assistant", "content": blocks })
        }
        MessageRole::System => {
            unreachable!("system messages are filtered before conversion")
        }
    }
}

fn text_and_thinking_blocks(m: &Message) -> Vec<Value> {
    let mut blocks: Vec<Value> = Vec::new();
    for part in &m.parts {
        match part {
            MessagePart::Text(t) if !t.is_empty() => {
                blocks.push(json!({ "type": "text", "text": t }));
            }
            MessagePart::Thinking(t) => {
                blocks.push(json!({ "type": "thinking", "thinking": t }));
            }
            _ => {}
        }
    }
    if blocks.is_empty() {
        blocks.push(json!({ "type": "text", "text": m.text() }));
    }
    blocks
}

fn build_api_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

fn map_stop_reason(reason: &str) -> StopReason {
    match reason {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    }
}

impl Provider for AnthropicProvider {
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
        let messages: Vec<Message> = messages.to_vec();
        let tools: Vec<ToolDef> = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_owned);

        Box::pin(async move {
            // Collect system text from both the explicit parameter and System-role messages.
            let mut system_parts: Vec<String> = Vec::new();
            if let Some(ref sys) = system_prompt {
                system_parts.push(sys.clone());
            }
            for m in &messages {
                if m.role == MessageRole::System && !m.text().is_empty() {
                    system_parts.push(m.text().to_string());
                }
            }

            let mut body = json!({
                "model": model,
                "max_tokens": DEFAULT_MAX_TOKENS,
                "stream": true,
                "messages": build_api_messages(&messages),
            });
            if !system_parts.is_empty() {
                body["system"] = json!(system_parts.join("\n\n"));
            }
            if !tools.is_empty() {
                body["tools"] = json!(build_api_tools(&tools));
            }

            let response = client
                .post(ANTHROPIC_API_URL)
                .header("x-api-key", &api_key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Anthropic API error {status}: {text}");
            }

            let (tx, rx) = tokio::sync::mpsc::channel::<anyhow::Result<ProviderEvent>>(64);
            tokio::spawn(async move {
                let mut sse = response.bytes_stream().eventsource();
                let mut index_to_tool_id: HashMap<u64, String> = HashMap::new();
                let mut pending_stop_reason: Option<StopReason> = None;

                while let Some(result) = sse.next().await {
                    match result {
                        Ok(event) => {
                            let data: Value = match serde_json::from_str(&event.data) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let event_type =
                                data.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "content_block_start" => {
                                    let index =
                                        data.get("index").and_then(Value::as_u64).unwrap_or(0);
                                    if let Some(block) = data.get("content_block") {
                                        let block_type = block
                                            .get("type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if block_type == "tool_use" {
                                            let id = block
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let name = block
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            index_to_tool_id.insert(index, id.clone());
                                            if tx
                                                .send(Ok(ProviderEvent::ToolCallStart { id, name }))
                                                .await
                                                .is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    let index =
                                        data.get("index").and_then(Value::as_u64).unwrap_or(0);
                                    if let Some(delta) = data.get("delta") {
                                        let delta_type = delta
                                            .get("type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        match delta_type {
                                            "text_delta" => {
                                                let text = delta
                                                    .get("text")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
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
                                            "thinking_delta" => {
                                                let text = delta
                                                    .get("thinking")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                if !text.is_empty()
                                                    && tx
                                                        .send(Ok(ProviderEvent::ThinkingDelta(
                                                            text.to_string(),
                                                        )))
                                                        .await
                                                        .is_err()
                                                {
                                                    return;
                                                }
                                            }
                                            "input_json_delta" => {
                                                let partial = delta
                                                    .get("partial_json")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                if let Some(id) = index_to_tool_id.get(&index) {
                                                    if tx
                                                        .send(Ok(ProviderEvent::ToolCallArgs {
                                                            id: id.clone(),
                                                            args: partial.to_string(),
                                                        }))
                                                        .await
                                                        .is_err()
                                                    {
                                                        return;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    let index =
                                        data.get("index").and_then(Value::as_u64).unwrap_or(0);
                                    if let Some(id) = index_to_tool_id.remove(&index) {
                                        if tx
                                            .send(Ok(ProviderEvent::ToolCallEnd { id }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                "message_delta" => {
                                    if let Some(delta) = data.get("delta") {
                                        if let Some(reason) =
                                            delta.get("stop_reason").and_then(|v| v.as_str())
                                        {
                                            pending_stop_reason = Some(map_stop_reason(reason));
                                        }
                                    }
                                }
                                "message_stop" => {
                                    let reason = pending_stop_reason.unwrap_or(StopReason::EndTurn);
                                    let _ = tx
                                        .send(Ok(ProviderEvent::Done {
                                            stop_reason: reason,
                                        }))
                                        .await;
                                    return;
                                }
                                "error" => {
                                    let msg = data
                                        .pointer("/error/message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown Anthropic error");
                                    let _ =
                                        tx.send(Ok(ProviderEvent::Error(msg.to_string()))).await;
                                    return;
                                }
                                _ => {} // ping, message_start, etc.
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(anyhow::anyhow!("SSE stream error: {e}"))).await;
                            return;
                        }
                    }
                }
            });

            let stream: ProviderStream = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
            Ok(stream)
        })
    }
}
