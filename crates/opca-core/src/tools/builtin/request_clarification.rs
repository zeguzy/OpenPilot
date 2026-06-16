//! `request_clarification` tool — lets a Task agent ask the user a
//! question when it is genuinely blocked and cannot proceed without
//! more information.
//!
//! When the Task calls this tool:
//! 1. The tool stores the question + options in a shared slot.
//! 2. The run loop detects the pending request, transitions to `Waiting`,
//!    and emits a Clarification notification via the heartbeat channel.
//! 3. The Orchestrator surfaces the question to the user.
//! 4. The user replies with `/answer <task-id> <choice>`.
//! 5. The Orchestrator injects the answer as a `SteeringMessage::Inject`.
//! 6. The Task transitions out of `Waiting` and resumes execution.
//!
//! See `design.md` §D6 (Clarification Protocol).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext};

/// The data captured when a Task calls `request_clarification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarificationRequest {
    pub question: String,
    pub options: Vec<String>,
}

/// Shared slot for the latest clarification request.
///
/// Cloned cheaply via `Arc`; the Task run loop checks this after each
/// tool batch to detect whether a clarification was requested.
pub type ClarificationStore = Arc<Mutex<Option<ClarificationRequest>>>;

/// Creates a fresh clarification store.
#[must_use]
pub fn new_clarification_store() -> ClarificationStore {
    Arc::new(Mutex::new(None))
}

pub struct RequestClarificationTool {
    store: ClarificationStore,
}

impl RequestClarificationTool {
    #[must_use]
    pub const fn new(store: ClarificationStore) -> Self {
        Self { store }
    }
}

const TOOL_NAME: &str = "request_clarification";
const TOOL_DESCRIPTION: &str = "\
Ask the user a question when you need more information to proceed. \
Use sparingly — only when truly blocked. \
Provide suggested options when possible to make answering easy.";

#[async_trait]
impl Tool for RequestClarificationTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        TOOL_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user."
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Suggested answers (optional). Makes it easier for the user to reply."
                }
            },
            "required": ["question"]
        })
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let question = args
            .get("question")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'question' argument"))?
            .to_string();

        let options: Vec<String> = args
            .get("options")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut guard = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(ClarificationRequest {
            question: question.clone(),
            options: options.clone(),
        });

        let opts_note = if options.is_empty() {
            String::new()
        } else {
            format!(" (options: {})", options.join(", "))
        };

        Ok(ToolResult {
            content: format!(
                "Clarification request queued. The user will be asked: {question}{opts_note}"
            ),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tool::{Tool, ToolContext};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn dummy_ctx() -> ToolContext {
        ToolContext {
            workspace_path: PathBuf::from("."),
            fs: Arc::new(crate::di::StdFileSystem),
            proc: Arc::new(crate::di::StdProcess),
            task_id: None,
        }
    }

    #[tokio::test]
    async fn stores_question_and_options() {
        let store = new_clarification_store();
        let tool = RequestClarificationTool::new(store.clone());

        let args = json!({
            "question": "JWT or sessions?",
            "options": ["JWT", "sessions"]
        });

        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("JWT or sessions?"));

        let guard = store.lock().unwrap();
        let req = guard.as_ref().unwrap();
        assert_eq!(req.question, "JWT or sessions?");
        assert_eq!(req.options, vec!["JWT", "sessions"]);
    }

    #[tokio::test]
    async fn works_without_options() {
        let store = new_clarification_store();
        let tool = RequestClarificationTool::new(store.clone());

        let args = json!({"question": "What version?"});
        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(!result.is_error);

        let guard = store.lock().unwrap();
        let req = guard.as_ref().unwrap();
        assert_eq!(req.question, "What version?");
        assert!(req.options.is_empty());
    }

    #[tokio::test]
    async fn errors_without_question() {
        let store = new_clarification_store();
        let tool = RequestClarificationTool::new(store);

        let result = tool.execute(&json!({}), &dummy_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn overwrites_previous_request() {
        let store = new_clarification_store();
        let tool = RequestClarificationTool::new(store.clone());

        tool.execute(&json!({"question": "first question?"}), &dummy_ctx())
            .await
            .unwrap();

        tool.execute(&json!({"question": "second question?"}), &dummy_ctx())
            .await
            .unwrap();

        let guard = store.lock().unwrap();
        assert_eq!(guard.as_ref().unwrap().question, "second question?");
    }

    #[test]
    fn new_store_starts_empty() {
        let store = new_clarification_store();
        let guard = store.lock().unwrap();
        assert!(guard.is_none());
    }
}
