use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::di::Clock;
use crate::focus::{FocusContract, Highlight, build_focus_prompt};
use crate::lifecycle::{Heartbeat, LifecycleTracker, TaskStatus, TodoSummary};
use crate::provider::{Message, Provider};
use crate::tools::builtin::{
    BashTool, ClarificationStore, EditTool, FindTool, GrepTool, LsTool, ReadTool,
    ReportHighlightTool, RequestClarificationTool, TodoStore, TodoWriteTool, WriteTool,
    new_clarification_store,
};
use crate::tools::{ToolContext, ToolRegistry};
use crate::workspace::Workspace;

use super::channels::{
    FollowupMessage, FollowupQueue, SteeringMessage, TaskHandle, TaskOutput, create_channels,
};
use super::run::{Phase, RunState};

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
    pub(super) run_state: RunState,
    pub(super) todo_store: TodoStore,
    pub(super) clarification_store: ClarificationStore,
    pub(super) project_context: Option<String>,
    #[cfg(feature = "sub-agents")]
    pub(super) dispatch_limits: Arc<std::sync::Mutex<crate::sub_agent::DispatchLimits>>,
    #[cfg(feature = "sub-agents")]
    pub(super) delegation_depth: usize,
    #[cfg(feature = "sub-agents")]
    pub(super) subtask_notification_queue:
        Option<crate::sub_agent::dispatch::SubTaskNotificationQueue>,
    #[cfg(feature = "sub-agents")]
    pub(super) pending_subtask_count: usize,
    #[cfg(feature = "sub-agents")]
    pub(super) waiting_since: Option<std::time::Instant>,
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

        let todo_store: TodoStore = Arc::new(std::sync::Mutex::new(Vec::new()));
        tools.register(Box::new(TodoWriteTool::new(todo_store.clone())));

        let clarification_store = new_clarification_store();
        tools.register(Box::new(RequestClarificationTool::new(
            clarification_store.clone(),
        )));

        tools.register(Box::new(ReadTool));
        tools.register(Box::new(WriteTool));
        tools.register(Box::new(EditTool));
        tools.register(Box::new(BashTool));
        tools.register(Box::new(GrepTool));
        tools.register(Box::new(FindTool));
        tools.register(Box::new(LsTool));

        #[cfg(feature = "sub-agents")]
        let dispatch_limits: Arc<std::sync::Mutex<crate::sub_agent::DispatchLimits>> = Arc::new(
            std::sync::Mutex::new(crate::sub_agent::DispatchLimits::default()),
        );

        let lifecycle = LifecycleTracker::new(id_string.clone(), clock)
            .with_heartbeat_channel(set.heartbeat_tx.clone());

        tracing::info!(
            prompt_version = crate::prompt_system::task::PROMPT_VERSION,
            task_id = %id_string,
            "prompt template loaded"
        );

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
            run_state: RunState::new(),
            todo_store,
            clarification_store,
            project_context: None,
            #[cfg(feature = "sub-agents")]
            dispatch_limits,
            #[cfg(feature = "sub-agents")]
            delegation_depth: 0,
            #[cfg(feature = "sub-agents")]
            subtask_notification_queue: None,
            #[cfg(feature = "sub-agents")]
            pending_subtask_count: 0,
            #[cfg(feature = "sub-agents")]
            waiting_since: None,
        };
        (task, handle)
    }

    /// Configures the Evidence Gate with `commands`. When set, the run
    /// loop captures a baseline at Task start and verifies before
    /// transitioning to `Delivered`.
    pub fn with_evidence_commands(&mut self, commands: Vec<String>) {
        if !commands.is_empty() {
            self.run_state
                .set_evidence_gate(super::evidence_gate::EvidenceGate::new(commands));
        }
    }

    /// Sets project-level context (from AGENTS.md) to be injected into
    /// the system prompt. This gives the Task awareness of coding
    /// conventions, build commands, and project architecture.
    pub fn set_project_context(&mut self, context: String) {
        self.project_context = Some(context);
    }

    /// Sets the delegation depth for sub-agent chains. Root tasks are
    /// depth 0; each child increments by 1. Also propagates the depth to
    /// the `dispatch_subtask` tool's limits so the tool enforces the
    /// correct ceiling.
    #[cfg(feature = "sub-agents")]
    pub fn with_delegation_depth(&mut self, depth: usize) {
        self.delegation_depth = depth;
        if let Ok(mut guard) = self.dispatch_limits.lock() {
            guard.current_depth = depth;
        }
    }

    /// Sets the notification queue the Task drains for sub-task completion
    /// notifications. Called by the Orchestrator after `Task::new()` but
    /// before spawning.
    #[cfg(feature = "sub-agents")]
    pub fn set_subtask_notification_queue(
        &mut self,
        queue: crate::sub_agent::dispatch::SubTaskNotificationQueue,
    ) {
        self.subtask_notification_queue = Some(queue);
    }

    /// Registers the `dispatch_subtask` tool using the Orchestrator's shared
    /// request queue. Called by the Orchestrator after `Task::new()` but
    /// before spawning. The tool shares the same `Arc<Mutex>` as the
    /// Orchestrator's `subtask_request_queue`.
    #[cfg(feature = "sub-agents")]
    pub fn register_dispatch_subtask_tool(
        &mut self,
        request_queue: Arc<std::sync::Mutex<Vec<crate::sub_agent::SubTaskRequest>>>,
    ) {
        use crate::sub_agent::DispatchSubtaskTool;
        self.tools.register(Box::new(DispatchSubtaskTool::new(
            request_queue,
            self.dispatch_limits.clone(),
        )));
    }

    /// Returns the delegation depth (0 for root tasks).
    #[cfg(feature = "sub-agents")]
    #[must_use]
    pub const fn delegation_depth(&self) -> usize {
        self.delegation_depth
    }

    /// Returns the current phase of the run loop.
    #[must_use]
    pub const fn current_phase(&self) -> Phase {
        self.run_state.current_phase
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
        let base = crate::prompt_system::task::build_task_prompt(self.run_state.current_phase);
        let focus = build_focus_prompt(&self.focus);
        let mut prompt = if focus.is_empty() {
            base
        } else {
            format!("{base}\n\n{focus}")
        };
        if let Some(ctx) = &self.project_context {
            prompt.push_str("\n\n## Project Context\n");
            prompt.push_str(ctx);
        }
        prompt
    }

    pub(super) fn push_output(&self, output: TaskOutput) {
        let _ = self.output_tx.send(output);
    }

    pub(super) fn push_heartbeat(&self, progress: f64, summary: &str) {
        let todo = self.todo_summary();
        let hb = Heartbeat {
            task_id: self.id.clone(),
            status: self.lifecycle.current(),
            progress: progress.clamp(0.0, 1.0),
            summary: summary.to_string(),
            timestamp: 0,
            todo,
            subtasks: Vec::new(),
        };
        let _ = self.heartbeat_tx.send(hb);
    }

    /// Syncs the shared todo store (written by `TodoWriteTool`) into
    /// `run_state.todo_list` so subsequent heartbeats reflect the latest
    /// list without locking the mutex on every read.
    pub(super) fn sync_todos(&mut self) {
        let guard = self
            .todo_store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.run_state.todo_list.clone_from(&guard);
    }

    fn todo_summary(&self) -> Option<TodoSummary> {
        let todos = &self.run_state.todo_list;
        if todos.is_empty() {
            return None;
        }
        let total = todos.len();
        let completed = todos.iter().filter(|t| t.status == "completed").count();
        let in_progress = todos
            .iter()
            .find(|t| t.status == "in_progress")
            .map(|t| t.content.clone());
        Some(TodoSummary {
            total,
            completed,
            in_progress,
        })
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
        let mut d = f.debug_struct("Task");
        d.field("id", &self.id)
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
            .field("run_state", &self.run_state.current_phase)
            .field(
                "todo_store",
                &self.todo_store.lock().map(|s| s.len()).unwrap_or(0),
            )
            .field(
                "clarification_pending",
                &self
                    .clarification_store
                    .lock()
                    .map(|s| s.is_some())
                    .unwrap_or(false),
            );
        #[cfg(feature = "sub-agents")]
        {
            d.field(
                "dispatch_limits",
                &self
                    .dispatch_limits
                    .lock()
                    .map(|l| l.current_depth)
                    .unwrap_or(0),
            );
            d.field("delegation_depth", &self.delegation_depth);
            d.field("pending_subtask_count", &self.pending_subtask_count);
        }
        d.finish_non_exhaustive()
    }
}
