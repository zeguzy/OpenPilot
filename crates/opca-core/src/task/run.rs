use std::collections::HashMap;

use tokio_stream::StreamExt;

use crate::lifecycle::TaskStatus;
use crate::provider::{Message, ProviderEvent, ToolCall};
use crate::tools::dispatch::dispatch_batch;

use super::channels::{SteeringMessage, TaskOutput};
use super::task::{Task, TaskOutcome};

const MAX_TURNS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SteeringOutcome {
    Continue,
    Cancelled,
}

impl Task {
    pub async fn run(&mut self, initial_input: &str) -> TaskOutcome {
        if let Err(e) = self.begin_lifecycle() {
            return TaskOutcome::Error(e);
        }
        self.active.push(Message::user(initial_input));

        loop {
            self.turn_count += 1;
            if self.turn_count > MAX_TURNS {
                return TaskOutcome::Error(format!("exceeded max turns ({MAX_TURNS})"));
            }

            if self.process_steering() == SteeringOutcome::Cancelled {
                let _ = self
                    .lifecycle
                    .transition(TaskStatus::Axed, 0.0, "cancelled");
                return TaskOutcome::Cancelled;
            }

            self.drain_followups();

            match self.run_turn().await {
                Ok(TurnVerdict::Continue) => {}
                Ok(TurnVerdict::Completed(msg)) => {
                    let _ = self
                        .lifecycle
                        .transition(TaskStatus::Delivered, 1.0, "delivered");
                    self.push_output(TaskOutput::StatusChanged {
                        status: TaskStatus::Delivered,
                        progress: 1.0,
                        summary: "delivered".to_string(),
                    });
                    self.push_output(TaskOutput::Done);
                    return TaskOutcome::Completed(msg);
                }
                Err(e) => {
                    let _ = self.lifecycle.transition(TaskStatus::Stuck, 0.0, "error");
                    self.push_output(TaskOutput::Done);
                    return TaskOutcome::Error(e);
                }
            }
        }
    }

    fn begin_lifecycle(&mut self) -> Result<(), String> {
        self.lifecycle
            .transition(TaskStatus::Waking, 0.0, "waking up")
            .map_err(|e| e.to_string())?;
        self.lifecycle
            .transition(TaskStatus::Pondering, 0.0, "pondering")
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn run_turn(&mut self) -> Result<TurnVerdict, String> {
        let system_prompt = self.build_system_prompt();
        let tools = self.tools.definitions();

        let stream = {
            let messages = self.active.clone();
            let provider = self.provider.clone();
            let prompt = if system_prompt.is_empty() {
                None
            } else {
                Some(system_prompt.as_str())
            };
            provider
                .stream(&messages, &tools, prompt)
                .await
                .map_err(|e| e.to_string())?
        };

        let (text, tool_calls, err) = self.collect_stream(stream).await;
        if let Some(err_msg) = err {
            return Err(err_msg);
        }

        let assistant_msg = Message::assistant_with_tools(text.clone(), tool_calls.clone());
        self.active.push(assistant_msg);

        if self.lifecycle.current() == TaskStatus::Pondering {
            let label = if tool_calls.is_empty() {
                "producing response"
            } else {
                "executing tools"
            };
            let _ = self.lifecycle.transition(TaskStatus::OnIt, 0.1, label);
        }

        if !tool_calls.is_empty() {
            self.execute_tools(tool_calls).await;
            self.push_heartbeat(0.5, "executed tools, continuing");
            return Ok(TurnVerdict::Continue);
        }

        self.on_turn_complete(&text);

        if !self.followup.is_empty() {
            if self.lifecycle.current() == TaskStatus::OnIt {
                let _ =
                    self.lifecycle
                        .transition(TaskStatus::Pondering, 0.3, "processing follow-up");
            }
            return Ok(TurnVerdict::Continue);
        }

        Ok(TurnVerdict::Completed(Message::assistant(text)))
    }

    async fn execute_tools(&mut self, tool_calls: Vec<ToolCall>) {
        let results = dispatch_batch(&self.tools, &tool_calls, &self.tool_ctx).await;
        for (call_id, result) in results {
            let tool_result = match result {
                Ok(r) => r,
                Err(e) => crate::provider::ToolResult {
                    content: e.to_string(),
                    is_error: true,
                },
            };
            let name = tool_calls
                .iter()
                .find(|tc| tc.id == call_id)
                .map(|tc| tc.name.clone())
                .unwrap_or_default();
            self.push_output(TaskOutput::ToolResult {
                name,
                success: !tool_result.is_error,
                summary: tool_result.content.chars().take(200).collect(),
            });
            self.active.push(Message::tool_result(call_id, tool_result));
        }
    }

    fn on_turn_complete(&self, text: &str) {
        let summary = if text.is_empty() {
            "turn complete"
        } else {
            text
        };
        self.push_heartbeat(0.8, summary);
    }

    fn process_steering(&mut self) -> SteeringOutcome {
        while let Ok(msg) = self.steering_rx.try_recv() {
            match msg {
                SteeringMessage::Cancel => return SteeringOutcome::Cancelled,
                SteeringMessage::Inject(m) => {
                    self.active.push(m);
                }
                SteeringMessage::UpdateFocus(update) => {
                    let _ = update.apply(&mut self.focus);
                }
            }
        }
        SteeringOutcome::Continue
    }

    async fn collect_stream(
        &self,
        mut stream: crate::provider::ProviderStream,
    ) -> (String, Vec<ToolCall>, Option<String>) {
        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut in_flight: HashMap<String, (String, String)> = HashMap::new();
        let mut err: Option<String> = None;

        while let Some(event) = stream.next().await {
            match event {
                Ok(ProviderEvent::TextDelta(delta)) => {
                    text.push_str(&delta);
                    self.push_output(TaskOutput::TextDelta(delta));
                }
                Ok(ProviderEvent::ToolCallStart { id, name }) => {
                    in_flight.insert(id, (name, String::new()));
                }
                Ok(ProviderEvent::ToolCallArgs { id, args }) => {
                    if let Some(entry) = in_flight.get_mut(&id) {
                        entry.1.push_str(&args);
                    }
                }
                Ok(ProviderEvent::ToolCallEnd { id }) => {
                    if let Some((name, raw_args)) = in_flight.remove(&id) {
                        let arguments = serde_json::from_str(&raw_args)
                            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                        tool_calls.push(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                        });
                        self.push_output(TaskOutput::ToolCall {
                            name,
                            args: arguments.to_string(),
                        });
                    }
                }
                Ok(ProviderEvent::Usage { .. }) => {}
                Ok(ProviderEvent::Done { .. }) => break,
                Ok(ProviderEvent::Error(message)) => {
                    err = Some(message);
                    break;
                }
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        (text, tool_calls, err)
    }
}

enum TurnVerdict {
    Continue,
    Completed(Message),
}

#[cfg(test)]
mod tests {
    use crate::focus::{FocusContract, FocusUpdate};

    use super::SteeringOutcome;

    #[test]
    fn steering_outcome_variants_are_distinct() {
        assert_ne!(SteeringOutcome::Continue, SteeringOutcome::Cancelled);
    }

    #[test]
    fn focus_update_apply_to_contract() {
        let mut focus = FocusContract::empty();
        let update = FocusUpdate::new().with_add(vec!["security".to_string()]);
        update.apply(&mut focus).unwrap();
        assert!(focus.contains("security"));
    }
}
