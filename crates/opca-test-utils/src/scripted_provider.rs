use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::stream;
use opca_core::provider::ToolResult;
use opca_core::provider::anyhow;
use opca_core::provider::{Message, Provider, ProviderEvent, ProviderStream, StopReason, ToolDef};

#[derive(Debug)]
enum ScriptedResponse {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult(ToolResult),
    Done(StopReason),
}

#[derive(Clone, Default)]
pub struct ScriptedProvider {
    responses: Arc<Mutex<VecDeque<ScriptedResponse>>>,
}

impl ScriptedProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn then_text(self, text: &str) -> Self {
        self.responses
            .lock()
            .expect("poisoned mutex")
            .push_back(ScriptedResponse::Text(text.to_string()));
        self
    }

    pub fn then_tool_call(self, name: &str, args: serde_json::Value) -> Self {
        let id = format!("call_{}", uuid::Uuid::new_v4());
        self.responses
            .lock()
            .expect("poisoned mutex")
            .push_back(ScriptedResponse::ToolCall {
                id,
                name: name.to_string(),
                args,
            });
        self
    }

    pub fn then_tool_result(self, content: &str, is_error: bool) -> Self {
        self.responses
            .lock()
            .expect("poisoned mutex")
            .push_back(ScriptedResponse::ToolResult(ToolResult {
                content: content.to_string(),
                is_error,
            }));
        self
    }

    pub fn then_done(self) -> Self {
        self.responses
            .lock()
            .expect("poisoned mutex")
            .push_back(ScriptedResponse::Done(StopReason::EndTurn));
        self
    }
}

impl Provider for ScriptedProvider {
    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[ToolDef],
        _system_prompt: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send>> {
        let responses = self.responses.clone();
        Box::pin(async move {
            let mut queue = responses.lock().expect("poisoned mutex");
            if queue.is_empty() {
                return Err(anyhow::anyhow!("ScriptedProvider exhausted"));
            }
            let mut events: Vec<anyhow::Result<ProviderEvent>> = Vec::new();
            while let Some(resp) = queue.pop_front() {
                match resp {
                    ScriptedResponse::Text(t) => {
                        events.push(Ok(ProviderEvent::TextDelta(t)));
                    }
                    ScriptedResponse::ToolCall { id, name, args } => {
                        events.push(Ok(ProviderEvent::ToolCallStart {
                            id: id.clone(),
                            name,
                        }));
                        events.push(Ok(ProviderEvent::ToolCallArgs {
                            id: id.clone(),
                            args: args.to_string(),
                        }));
                        events.push(Ok(ProviderEvent::ToolCallEnd { id }));
                    }
                    ScriptedResponse::ToolResult(result) => {
                        drop(result);
                    }
                    ScriptedResponse::Done(reason) => {
                        events.push(Ok(ProviderEvent::Done {
                            stop_reason: reason,
                        }));
                        break;
                    }
                }
            }
            drop(queue);
            let s: ProviderStream = Box::pin(stream::iter(events));
            Ok(s)
        })
    }
}

impl std::fmt::Debug for ScriptedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.responses.lock().expect("poisoned mutex").len();
        f.debug_struct("ScriptedProvider")
            .field("pending_responses", &len)
            .finish()
    }
}
