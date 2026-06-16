//! `dispatch_subtask` tool and supporting types.
//!
//! When a Task calls `dispatch_subtask`, the tool validates the delegation
//! depth and parallel limits, then constructs a [`SubTaskRequest`] that the
//! Orchestrator consumes to spawn a child Task with `parent_task_id` set.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::provider::{ToolEffects, ToolResult};
use crate::tools::tool::{Tool, ToolContext};

/// Workspace scope for a sub-task (D7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubTaskScope {
    /// Share the parent's workspace path (read-write). Default.
    #[default]
    Inherited,
    /// Fresh isolated workspace from main branch.
    Isolated,
}

/// Request to dispatch a sub-task, sent from the parent Task's tool
/// execution to the Orchestrator.
#[derive(Debug, Clone)]
pub struct SubTaskRequest {
    pub description: String,
    pub focus: Vec<String>,
    pub workspace_mode: SubTaskScope,
    pub parent_id: String,
    pub parent_workspace_path: PathBuf,
    pub delegation_depth: usize,
}

/// Ticket returned to the parent Task when a sub-task is dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubTaskTicket {
    pub sub_task_id: String,
}

/// Structured result delivered to the parent when a sub-task completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubTaskResult {
    pub task_id: String,
    pub summary: String,
    pub artifacts: Vec<PathBuf>,
}

/// Verdict for a completed sub-task, used in the completion notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubTaskVerdict {
    Delivered,
    Failed,
}

/// Notification sent to a parent Task when a sub-task finishes.
#[derive(Debug, Clone)]
pub enum SubTaskNotification {
    Completed {
        sub_task_id: String,
        result: SubTaskResult,
        verdict: SubTaskVerdict,
    },
    Failed {
        sub_task_id: String,
        reason: String,
    },
}

/// Shared queue for sub-task notifications delivered to a parent Task.
pub type SubTaskNotificationQueue = Arc<Mutex<Vec<SubTaskNotification>>>;

/// Creates a fresh notification queue.
#[must_use]
pub fn new_notification_queue() -> SubTaskNotificationQueue {
    Arc::new(Mutex::new(Vec::new()))
}

/// Tool definition constants for `dispatch_subtask`.
pub struct DispatchSubtaskToolDef;

impl DispatchSubtaskToolDef {
    #[must_use]
    pub const fn name() -> &'static str {
        "dispatch_subtask"
    }

    #[must_use]
    pub const fn description() -> &'static str {
        "Dispatch a sub-task to handle part of your work. \
         The sub-task runs in its own context with an inherited workspace. \
         Use for parallelizable sub-problems or independent units of work."
    }

    #[must_use]
    pub fn parameters_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "What the sub-task should do."
                },
                "focus": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Focus dimensions for the sub-task (subset of parent's contract)."
                },
                "workspace_mode": {
                    "type": "string",
                    "enum": ["inherited", "isolated"],
                    "description": "Workspace strategy. 'inherited' (default) shares the parent workspace; 'isolated' creates a fresh worktree."
                }
            },
            "required": ["description"]
        })
    }
}

/// Configuration limits checked at dispatch time.
#[derive(Debug, Clone, Copy)]
pub struct DispatchLimits {
    pub depth_limit: usize,
    pub parallel_limit: usize,
    pub active_subtask_count: usize,
    pub current_depth: usize,
}

impl Default for DispatchLimits {
    fn default() -> Self {
        Self {
            depth_limit: 2,
            parallel_limit: 3,
            active_subtask_count: 0,
            current_depth: 0,
        }
    }
}

/// `dispatch_subtask` tool implementation.
///
/// The tool does NOT spawn the child Task directly. Instead it validates
/// limits and returns a [`ToolResult`] indicating success or an error
/// message. The actual sub-task spawn is handled by the Orchestrator
/// via the [`SubTaskRequest`] channel.
pub struct DispatchSubtaskTool {
    request_queue: Arc<Mutex<Vec<SubTaskRequest>>>,
    limits: Arc<Mutex<DispatchLimits>>,
}

impl DispatchSubtaskTool {
    #[must_use]
    pub const fn new(
        request_queue: Arc<Mutex<Vec<SubTaskRequest>>>,
        limits: Arc<Mutex<DispatchLimits>>,
    ) -> Self {
        Self {
            request_queue,
            limits,
        }
    }

    /// Sets the active subtask count (called by the run loop before
    /// executing the tool batch).
    pub fn set_active_count(&self, count: usize) {
        let mut guard = self
            .limits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.active_subtask_count = count;
    }

    fn enqueue_request(&self, request: SubTaskRequest) -> String {
        let mut guard = self
            .request_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ticket_id = format!("subtask-{}", guard.len());
        guard.push(request);
        ticket_id
    }
}

const TOOL_NAME: &str = "dispatch_subtask";

#[async_trait]
impl Tool for DispatchSubtaskTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        DispatchSubtaskToolDef::description()
    }

    fn parameters_schema(&self) -> Value {
        DispatchSubtaskToolDef::parameters_schema()
    }

    fn effects(&self) -> ToolEffects {
        ToolEffects::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing 'description' argument"))?
            .to_string();

        let focus: Vec<String> = args
            .get("focus")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let workspace_mode = args
            .get("workspace_mode")
            .and_then(Value::as_str)
            .map(|s| match s {
                "isolated" => SubTaskScope::Isolated,
                _ => SubTaskScope::Inherited,
            })
            .unwrap_or_default();

        let limits = self
            .limits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let current_depth = limits.current_depth;
        let depth_limit = limits.depth_limit;
        let parallel_limit = limits.parallel_limit;
        let active_count = limits.active_subtask_count;
        drop(limits);

        if current_depth >= depth_limit {
            return Ok(ToolResult {
                content: format!(
                    "Maximum delegation depth ({depth_limit}) reached. Handle this directly."
                ),
                is_error: true,
            });
        }

        if active_count >= parallel_limit {
            return Ok(ToolResult {
                content: format!(
                    "Maximum concurrent sub-tasks ({parallel_limit}) reached; wait for one to complete."
                ),
                is_error: true,
            });
        }

        let request = SubTaskRequest {
            description: description.clone(),
            focus: focus.clone(),
            workspace_mode: workspace_mode.clone(),
            parent_id: ctx.task_id.clone().unwrap_or_default(),
            parent_workspace_path: ctx.workspace_path.clone(),
            delegation_depth: current_depth,
        };

        let ticket_id = self.enqueue_request(request);

        let mode_str = match workspace_mode {
            SubTaskScope::Inherited => "inherited",
            SubTaskScope::Isolated => "isolated",
        };

        Ok(ToolResult {
            content: format!(
                "Sub-task dispatched (ticket: {ticket_id}). Description: \"{description}\". \
                 Workspace: {mode_str}. Focus: {focus:?}. \
                 You will be notified when it completes."
            ),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn dummy_ctx() -> ToolContext {
        ToolContext {
            workspace_path: PathBuf::from("/workspace"),
            fs: Arc::new(crate::di::StdFileSystem),
            proc: Arc::new(crate::di::StdProcess),
            task_id: None,
        }
    }

    fn make_tool() -> DispatchSubtaskTool {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits::default()));
        DispatchSubtaskTool::new(queue, limits)
    }

    #[test]
    fn tool_def_constants() {
        assert_eq!(DispatchSubtaskToolDef::name(), "dispatch_subtask");
        assert!(!DispatchSubtaskToolDef::description().is_empty());
        let schema = DispatchSubtaskToolDef::parameters_schema();
        assert!(schema.get("properties").is_some());
        assert!(
            schema["properties"]["workspace_mode"]["enum"]
                .as_array()
                .is_some()
        );
    }

    #[test]
    fn subtask_scope_default_is_inherited() {
        assert_eq!(SubTaskScope::default(), SubTaskScope::Inherited);
    }

    #[tokio::test]
    async fn dispatch_creates_request_when_limits_ok() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits::default()));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({
            "description": "Find all uses of deprecated_api()",
            "focus": ["diff-sanity"],
            "workspace_mode": "inherited"
        });

        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Sub-task dispatched"));

        let guard = queue.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].description, "Find all uses of deprecated_api()");
        assert_eq!(guard[0].workspace_mode, SubTaskScope::Inherited);
        assert_eq!(guard[0].focus, vec!["diff-sanity".to_string()]);
    }

    #[tokio::test]
    async fn dispatch_rejected_at_depth_limit() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits {
            depth_limit: 0,
            parallel_limit: 3,
            active_subtask_count: 0,
            current_depth: 0,
        }));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({"description": "test"});
        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Maximum delegation depth"));

        let guard = queue.lock().unwrap();
        assert!(guard.is_empty(), "no request should be enqueued");
    }

    #[tokio::test]
    async fn dispatch_rejected_at_parallel_limit() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits {
            depth_limit: 2,
            parallel_limit: 3,
            active_subtask_count: 3,
            current_depth: 0,
        }));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({"description": "test"});
        let result = tool.execute(&args, &dummy_ctx()).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Maximum concurrent sub-tasks"));

        let guard = queue.lock().unwrap();
        assert!(guard.is_empty());
    }

    #[tokio::test]
    async fn dispatch_defaults_workspace_to_inherited() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits::default()));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({"description": "test"});
        tool.execute(&args, &dummy_ctx()).await.unwrap();

        let guard = queue.lock().unwrap();
        assert_eq!(guard[0].workspace_mode, SubTaskScope::Inherited);
    }

    #[tokio::test]
    async fn dispatch_with_isolated_workspace() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits::default()));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({
            "description": "destructive refactor",
            "workspace_mode": "isolated"
        });
        tool.execute(&args, &dummy_ctx()).await.unwrap();

        let guard = queue.lock().unwrap();
        assert_eq!(guard[0].workspace_mode, SubTaskScope::Isolated);
    }

    #[tokio::test]
    async fn dispatch_errors_without_description() {
        let tool = make_tool();
        let result = tool.execute(&json!({}), &dummy_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dispatch_captures_parent_workspace_path() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(Mutex::new(DispatchLimits::default()));
        let tool = DispatchSubtaskTool::new(queue.clone(), limits);

        let args = json!({"description": "test"});
        tool.execute(&args, &dummy_ctx()).await.unwrap();

        let guard = queue.lock().unwrap();
        assert_eq!(guard[0].parent_workspace_path, PathBuf::from("/workspace"));
    }

    #[test]
    fn notification_queue_starts_empty() {
        let q = new_notification_queue();
        assert!(q.lock().unwrap().is_empty());
    }

    #[test]
    fn subtask_result_has_artifacts() {
        let result = SubTaskResult {
            task_id: "sub-1".to_string(),
            summary: "done".to_string(),
            artifacts: vec![PathBuf::from("src/auth.rs")],
        };
        assert_eq!(result.artifacts.len(), 1);
    }

    #[test]
    fn subtask_scope_serializes_lowercase() {
        let json = serde_json::to_string(&SubTaskScope::Inherited).unwrap();
        assert_eq!(json, "\"inherited\"");
        let json = serde_json::to_string(&SubTaskScope::Isolated).unwrap();
        assert_eq!(json, "\"isolated\"");
    }
}
