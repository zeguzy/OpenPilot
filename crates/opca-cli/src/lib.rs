#![forbid(unsafe_code)]

pub mod commands;
pub mod mock;
pub mod real;
pub mod repl;
pub mod tui;

pub use commands::{SlashCommand, SlashError};
pub use mock::MockOrchestrator;
pub use real::RealOrchestrator;
pub use repl::{BufferOutput, Output, Repl, StdOutput};

use std::sync::Arc;

use opca_core::lifecycle::TaskStatus;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub summary: String,
    pub files_modified: usize,
}

/// Sub-task info for the `/subtasks` slash command (behind the
/// `sub-agents` feature).
#[cfg(feature = "sub-agents")]
#[derive(Debug, Clone)]
pub struct SubTaskInfo {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub summary: String,
}

impl TaskInfo {
    #[must_use]
    pub fn pending_review(&self) -> bool {
        self.status == TaskStatus::Delivered
    }

    #[must_use]
    pub const fn active(&self) -> bool {
        !matches!(
            self.status,
            TaskStatus::Delivered | TaskStatus::Stuck | TaskStatus::Axed | TaskStatus::Archived
        )
    }
}

#[derive(Debug, Clone)]
pub enum Notification {
    Completed {
        task_id: String,
        description: String,
        files_modified: usize,
    },
    StatusChanged {
        task_id: String,
        status: TaskStatus,
        summary: String,
    },
    /// A Task has entered `Waiting` and needs user input (D6).
    ///
    /// Emitted when the Task calls `request_clarification` or otherwise
    /// transitions to `Waiting` with a question. The CLI renders a banner
    /// prompting the user to reply with `/answer <task-id> <response>`.
    Clarification {
        task_id: String,
        question: String,
        /// Suggested answers the user can pick from (may be empty).
        options: Vec<String>,
        /// Seconds before the Orchestrator auto-proceeds with a best guess.
        timeout_secs: u64,
    },
}

#[derive(Debug)]
pub enum Reply {
    Dispatched {
        task_id: String,
        description: String,
    },
    Foreground(String),
    Acknowledged(String),
    Error(String),
    Nothing,
}

pub trait OrchestratorApi: Send + Sync {
    fn handle_message(&self, message: &str) -> Reply;
    fn dispatch(&self, description: &str) -> String;
    fn list_tasks(&self) -> Vec<TaskInfo>;
    fn task_status(&self, task_id: &str) -> Option<TaskInfo>;
    fn accept(&self, task_id: &str) -> Result<(), String>;
    fn reject(&self, task_id: &str, feedback: Option<&str>) -> Result<(), String>;
    /// Answers a clarification request from a `Waiting` Task.
    /// Forwards `choice` as a `SteeringMessage::Inject` and resumes the Task.
    fn answer_task(&self, task_id: &str, choice: &str) -> Result<(), String>;
    fn pending_review_count(&self) -> usize;
    fn subscribe(&self) -> UnboundedReceiver<Notification>;
    fn stream_foreground(
        &self,
        message: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tui::app::StreamEvent>,
    );

    /// Starts a continuation chain rooted at a freshly dispatched Task.
    ///
    /// Returns the chain ID. `max_iterations` and `budget` (USD) optionally
    /// override the configured defaults.
    fn start_continuation(
        &self,
        prompt: &str,
        max_iterations: Option<u32>,
        budget: Option<f64>,
    ) -> String;

    /// Terminates one chain (`chain_id`) or every active chain (when
    /// `chain_id` is `"all"`, case-insensitive). Returns the number of
    /// chains that were actually terminated.
    fn stop_continuation(&self, chain_id: &str) -> Result<usize, String>;

    /// Formats a human-readable status report.
    ///
    /// Pass `None` for an overview of every active chain, or `Some(id)` for
    /// a single chain's detail.
    fn continuation_status(&self, chain_id: Option<&str>) -> String;

    /// Lists sub-tasks of a parent task (behind the `sub-agents` feature).
    ///
    /// Pass `None` for all sub-tasks across all parents, or `Some(id)` for
    /// a specific parent's children.
    #[cfg(feature = "sub-agents")]
    fn list_subtasks(&self, parent_task_id: Option<&str>) -> Vec<SubTaskInfo>;
}

#[derive(Clone)]
pub struct ReplContext {
    pub orchestrator: Arc<dyn OrchestratorApi>,
    pub output: Arc<dyn Output>,
}

impl ReplContext {
    #[must_use]
    pub fn new(orchestrator: Arc<dyn OrchestratorApi>, output: Arc<dyn Output>) -> Self {
        Self {
            orchestrator,
            output,
        }
    }
}
