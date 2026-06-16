use crate::di::Clock;
use crate::lifecycle::status::{TaskStatus, TransitionError, transition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// Summary of the Task's todo list, surfaced in Layer 1 heartbeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSummary {
    pub total: usize,
    pub completed: usize,
    pub in_progress: Option<String>,
}

/// Aggregated snapshot of a sub-task's Layer 1 state, folded into the
/// parent Task's heartbeat for unified observability.
///
/// Populated by the sub-agent aggregation logic (behind the `sub-agents`
/// feature). Without the feature, the `Heartbeat::subtasks` vector is
/// always empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubTaskHeartbeat {
    pub id: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub in_progress_todo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub task_id: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub summary: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub todo: Option<TodoSummary>,
    /// Folded sub-task heartbeats. Always empty without the `sub-agents`
    /// feature; populated by the aggregation layer when enabled.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subtasks: Vec<SubTaskHeartbeat>,
}

pub struct LifecycleTracker {
    task_id: String,
    current: TaskStatus,
    heartbeat_tx: Option<UnboundedSender<Heartbeat>>,
    clock: Arc<dyn Clock>,
}

impl LifecycleTracker {
    #[must_use]
    pub fn new(task_id: impl Into<String>, clock: Arc<dyn Clock>) -> Self {
        Self {
            task_id: task_id.into(),
            current: TaskStatus::Sleeping,
            heartbeat_tx: None,
            clock,
        }
    }

    #[must_use]
    pub fn with_heartbeat_channel(mut self, tx: UnboundedSender<Heartbeat>) -> Self {
        self.heartbeat_tx = Some(tx);
        self
    }

    #[must_use]
    pub const fn current(&self) -> TaskStatus {
        self.current
    }

    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Returns the clock used for heartbeat timestamps. Exposed so a parent
    /// Task can hand the same clock to a child Task it spawns inline.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    pub fn transition(
        &mut self,
        to: TaskStatus,
        progress: f64,
        summary: &str,
    ) -> Result<Heartbeat, TransitionError> {
        transition(self.current, to).map(|new_status| {
            self.current = new_status;
            let heartbeat = Heartbeat {
                task_id: self.task_id.clone(),
                status: new_status,
                progress: progress.clamp(0.0, 1.0),
                summary: summary.to_string(),
                timestamp: unix_timestamp(self.clock.as_ref()),
                todo: None,
                subtasks: Vec::new(),
            };
            if let Some(tx) = &self.heartbeat_tx {
                let _ = tx.send(heartbeat.clone());
            }
            heartbeat
        })
    }
}

fn unix_timestamp(clock: &dyn Clock) -> u64 {
    clock
        .now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
