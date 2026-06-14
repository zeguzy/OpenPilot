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
    fn pending_review_count(&self) -> usize;
    fn subscribe(&self) -> UnboundedReceiver<Notification>;
    fn stream_foreground(
        &self,
        message: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tui::app::StreamEvent>,
    );
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
