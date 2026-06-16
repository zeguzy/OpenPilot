//! `TodoWrite` tool — lets the Task agent maintain a structured todo list
//! for multi-step work.
//!
//! The tool stores items in a shared `Arc<Mutex<Vec<TodoItem>>>` that
//! the Task reads after each tool batch to sync into `RunState::todo_list`
//! and to populate the heartbeat's `todo` summary.
//!
//! See `design.md` §D2 (phase protocol) and `specs/task-lifecycle/spec.md`
//! for the `TodoWrite` requirement contract (G7).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::task::run::TodoItem;
use crate::tools::tool::{Tool, ToolContext};

/// Shared todo-list store. Cloned cheaply via `Arc`; the inner `Mutex`
/// is held only for microseconds during tool execution.
pub type TodoStore = Arc<Mutex<Vec<TodoItem>>>;

/// Tool definition constants for `todowrite`.
pub struct TodoWriteToolDef;

impl TodoWriteToolDef {
    #[must_use]
    pub const fn name() -> &'static str {
        "todowrite"
    }

    #[must_use]
    pub const fn description() -> &'static str {
        "Create or update the task todo list. Use at the start of multi-step work (3+ steps). \
         Mark items in_progress as you start them, completed when done."
    }

    #[must_use]
    pub fn parameters_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["items"]
        })
    }
}

/// Tool implementation for `todowrite`. Writes the full todo list
/// atomically to the shared store.
pub struct TodoWriteTool {
    store: TodoStore,
}

impl TodoWriteTool {
    /// Creates a new tool wired to `store`. The same `TodoStore` must
    /// be held by the `Task` so it can sync the list into `RunState`.
    #[must_use]
    pub const fn new(store: TodoStore) -> Self {
        Self { store }
    }
}

const VALID_STATUSES: &[&str] = &["pending", "in_progress", "completed", "cancelled"];
const VALID_PRIORITIES: &[&str] = &["high", "medium", "low"];

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        TodoWriteToolDef::name()
    }

    fn description(&self) -> &str {
        TodoWriteToolDef::description()
    }

    fn parameters_schema(&self) -> Value {
        TodoWriteToolDef::parameters_schema()
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Append
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let items_arr = args
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("missing 'items' array argument"))?;

        let mut parsed: Vec<TodoItem> = Vec::with_capacity(items_arr.len());
        for (i, item) in items_arr.iter().enumerate() {
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("items[{i}] missing 'content'"))?
                .to_string();

            let status = item
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("items[{i}] missing 'status'"))?;
            if !VALID_STATUSES.contains(&status) {
                anyhow::bail!(
                    "items[{i}] invalid status '{status}'; expected one of {VALID_STATUSES:?}"
                );
            }

            let priority = item
                .get("priority")
                .and_then(Value::as_str)
                .unwrap_or("medium");
            if !VALID_PRIORITIES.contains(&priority) {
                anyhow::bail!(
                    "items[{i}] invalid priority '{priority}'; expected one of {VALID_PRIORITIES:?}"
                );
            }

            parsed.push(TodoItem {
                content,
                status: status.to_string(),
                priority: priority.to_string(),
            });
        }

        let count = parsed.len();
        let mut guard = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = parsed;

        Ok(ToolResult {
            content: format!("todo list updated ({count} items)"),
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
    async fn todowrite_stores_items() {
        let store: TodoStore = Arc::new(Mutex::new(Vec::new()));
        let tool = TodoWriteTool::new(store.clone());

        let args = json!({
            "items": [
                { "content": "read files", "status": "completed" },
                { "content": "write module", "status": "in_progress" },
                { "content": "run tests", "status": "pending" }
            ]
        });

        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("3 items"));

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 3);
        assert_eq!(guard[0].content, "read files");
        assert_eq!(guard[0].status, "completed");
        assert_eq!(guard[1].status, "in_progress");
        assert_eq!(guard[2].status, "pending");
        assert_eq!(guard[0].priority, "medium");
    }

    #[tokio::test]
    async fn todowrite_replaces_full_list() {
        let store: TodoStore = Arc::new(Mutex::new(vec![TodoItem {
            content: "old".to_string(),
            status: "completed".to_string(),
            priority: "low".to_string(),
        }]));

        let tool = TodoWriteTool::new(store.clone());
        let args = json!({
            "items": [
                { "content": "new task", "status": "pending", "priority": "high" }
            ]
        });

        tool.execute(&args, &dummy_ctx()).await.unwrap();

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].content, "new task");
        assert_eq!(guard[0].priority, "high");
    }

    #[tokio::test]
    async fn todowrite_rejects_invalid_status() {
        let store: TodoStore = Arc::new(Mutex::new(Vec::new()));
        let tool = TodoWriteTool::new(store);

        let args = json!({
            "items": [
                { "content": "x", "status": "bogus" }
            ]
        });

        let result = tool.execute(&args, &dummy_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn todowrite_rejects_missing_items() {
        let store: TodoStore = Arc::new(Mutex::new(Vec::new()));
        let tool = TodoWriteTool::new(store);

        let result = tool.execute(&json!({}), &dummy_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn todowrite_accepts_empty_list() {
        let store: TodoStore = Arc::new(Mutex::new(vec![TodoItem {
            content: "stale".to_string(),
            status: "pending".to_string(),
            priority: "medium".to_string(),
        }]));

        let tool = TodoWriteTool::new(store.clone());
        let args = json!({ "items": [] });
        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(result.content.contains("0 items"));

        let guard = store.lock().unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn tool_def_constants() {
        assert_eq!(TodoWriteToolDef::name(), "todowrite");
        assert!(!TodoWriteToolDef::description().is_empty());
        let schema = TodoWriteToolDef::parameters_schema();
        assert!(schema.get("properties").is_some());
    }
}
