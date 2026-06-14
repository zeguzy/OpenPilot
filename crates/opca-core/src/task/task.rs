use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::di::Clock;
use crate::focus::{FocusContract, Highlight, build_focus_prompt};
use crate::lifecycle::{Heartbeat, LifecycleTracker, TaskStatus};
use crate::provider::{Message, Provider};
use crate::tools::builtin::ReportHighlightTool;
use crate::tools::{ToolContext, ToolRegistry};
use crate::workspace::Workspace;

use super::channels::{
    FollowupMessage, FollowupQueue, SteeringMessage, TaskHandle, TaskOutput, create_channels,
};

#[derive(Debug, Clone, PartialEq)]
pub enum TaskOutcome {
    Completed(Message),
    Cancelled,
    Error(String),
}

pub struct Task {
    pub(super) id: String,
    pub(super) provider: Arc<dyn Provider>,
    pub(super) workspace: Box<dyn Workspace>,
    pub(super) active: Vec<Message>,
    pub(super) focus: FocusContract,
    pub(super) tools: ToolRegistry,
    pub(super) tool_ctx: ToolContext,
    pub(super) lifecycle: LifecycleTracker,
    pub(super) steering_rx: UnboundedReceiver<SteeringMessage>,
    pub(super) heartbeat_tx: UnboundedSender<Heartbeat>,
    pub(super) highlight_tx: UnboundedSender<Highlight>,
    pub(super) output_tx: UnboundedSender<TaskOutput>,
    pub(super) followup: FollowupQueue,
    pub(super) turn_count: u64,
}

impl Task {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        provider: Arc<dyn Provider>,
        workspace: Box<dyn Workspace>,
        focus: FocusContract,
        mut tools: ToolRegistry,
        tool_ctx: ToolContext,
        clock: Arc<dyn Clock>,
    ) -> (Self, TaskHandle) {
        let id_string = id.into();
        let (handle, set) = create_channels();

        let focus_arc = Arc::new(focus.clone());
        tools.register(Box::new(ReportHighlightTool::new(
            focus_arc,
            set.highlight_tx.clone(),
        )));

        let lifecycle = LifecycleTracker::new(id_string.clone(), clock)
            .with_heartbeat_channel(set.heartbeat_tx.clone());

        let task = Self {
            id: id_string,
            provider,
            workspace,
            active: Vec::new(),
            focus,
            tools,
            tool_ctx,
            lifecycle,
            steering_rx: set.steering_rx,
            heartbeat_tx: set.heartbeat_tx,
            highlight_tx: set.highlight_tx,
            output_tx: set.output_tx,
            followup: FollowupQueue::new(),
            turn_count: 0,
        };
        (task, handle)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn active_messages(&self) -> &[Message] {
        &self.active
    }

    pub const fn focus(&self) -> &FocusContract {
        &self.focus
    }

    pub const fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub const fn turn_count(&self) -> u64 {
        self.turn_count
    }

    pub const fn lifecycle_current(&self) -> TaskStatus {
        self.lifecycle.current()
    }

    pub fn workspace(&self) -> &dyn Workspace {
        self.workspace.as_ref()
    }

    /// Mutable borrow of the workspace (used by the `CompletionPipeline` after
    /// the agent loop finishes so it can freeze and merge the workspace).
    pub fn workspace_mut(&mut self) -> &mut dyn Workspace {
        self.workspace.as_mut()
    }

    pub const fn highlight_sender(&self) -> &UnboundedSender<Highlight> {
        &self.highlight_tx
    }

    pub fn push_followup(&mut self, msg: Message) {
        self.followup.push(FollowupMessage::User(msg));
    }

    #[must_use]
    pub fn followup_len(&self) -> usize {
        self.followup.len()
    }

    pub(super) fn build_system_prompt(&self) -> String {
        let base = crate::provider::task_prompt();
        let focus = build_focus_prompt(&self.focus);
        if focus.is_empty() {
            base.to_string()
        } else {
            format!("{base}\n\n{focus}")
        }
    }

    pub(super) fn push_output(&self, output: TaskOutput) {
        let _ = self.output_tx.send(output);
    }

    pub(super) fn push_heartbeat(&self, progress: f64, summary: &str) {
        let hb = Heartbeat {
            task_id: self.id.clone(),
            status: self.lifecycle.current(),
            progress: progress.clamp(0.0, 1.0),
            summary: summary.to_string(),
            timestamp: 0,
        };
        let _ = self.heartbeat_tx.send(hb);
    }

    pub(super) fn drain_followups(&mut self) -> bool {
        let followups = self.followup.drain();
        let had_any = !followups.is_empty();
        for FollowupMessage::User(m) in followups {
            self.active.push(m);
        }
        had_any
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("provider", &"..")
            .field("workspace", &"..")
            .field("active", &self.active)
            .field("focus", &self.focus)
            .field("tools", &self.tools.len())
            .field("tool_ctx", &"..")
            .field("lifecycle", &self.lifecycle.current())
            .field("steering_rx", &"..")
            .field("heartbeat_tx", &"..")
            .field("highlight_tx", &"..")
            .field("output_tx", &"..")
            .field("followup", &self.followup.len())
            .field("turn_count", &self.turn_count)
            .finish()
    }
}
