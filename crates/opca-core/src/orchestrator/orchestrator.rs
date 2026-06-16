#[cfg(feature = "sub-agents")]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::continuation::{ContinuationCoordinator, DefaultContinuationPolicy};
use crate::di::{Clock, FileSystem, Process};
use crate::focus::{FocusContract, FocusUpdate, Highlight};
use crate::lifecycle::{Heartbeat, TaskStatus};
use crate::memory::{EventKind, Memory, OrchestratorEvent, RecallQuery, extract_keywords};
use crate::provider::{Message, Provider};
use crate::task::{SteeringMessage, Task, TaskHandle, TaskOutcome};
use crate::tools::{ToolContext, ToolRegistry};
use crate::workspace::WorkspaceManager;

use super::registry::{ContextSnapshot, SubTaskRecord, TaskEntry, TaskRegistry};

pub struct Orchestrator {
    provider: Arc<dyn Provider>,
    memory: Memory<OrchestratorEvent>,
    tasks: TaskRegistry,
    workspace_manager: WorkspaceManager,
    clock: Arc<dyn Clock>,
    project_path: PathBuf,
    fs: Arc<dyn FileSystem>,
    proc: Arc<dyn Process>,
    heartbeat_tx: UnboundedSender<(String, Heartbeat)>,
    heartbeat_rx: UnboundedReceiver<(String, Heartbeat)>,
    highlight_tx: UnboundedSender<(String, Highlight)>,
    highlight_rx: UnboundedReceiver<(String, Highlight)>,
    output_tx: UnboundedSender<(String, crate::task::TaskOutput)>,
    output_rx: UnboundedReceiver<(String, crate::task::TaskOutput)>,
    prefetch_cache: Arc<Mutex<Vec<OrchestratorEvent>>>,
    task_counter: u64,
    continuation: ContinuationCoordinator,
    #[cfg(feature = "sub-agents")]
    subtask_request_queue: Arc<Mutex<Vec<crate::sub_agent::SubTaskRequest>>>,
    #[cfg(feature = "sub-agents")]
    subtask_notifications: HashMap<String, crate::sub_agent::dispatch::SubTaskNotificationQueue>,
}

impl Orchestrator {
    #[must_use]
    pub fn new(
        provider: Arc<dyn Provider>,
        project_path: PathBuf,
        clock: Arc<dyn Clock>,
        fs: Arc<dyn FileSystem>,
        proc: Arc<dyn Process>,
    ) -> Self {
        let memory = Memory::<OrchestratorEvent>::new_in_memory(10_000)
            .expect("in-memory store should succeed");
        let (heartbeat_tx, heartbeat_rx) = mpsc::unbounded_channel();
        let (highlight_tx, highlight_rx) = mpsc::unbounded_channel();
        let (output_tx, output_rx) = mpsc::unbounded_channel();
        tracing::info!(
            prompt_version = crate::prompt_system::orchestrator::PROMPT_VERSION,
            "prompt template loaded"
        );
        Self {
            provider,
            memory,
            tasks: TaskRegistry::new(),
            workspace_manager: WorkspaceManager::new(),
            clock,
            project_path,
            fs,
            proc,
            heartbeat_tx,
            heartbeat_rx,
            highlight_tx,
            highlight_rx,
            output_tx,
            output_rx,
            prefetch_cache: Arc::new(Mutex::new(Vec::new())),
            task_counter: 0,
            continuation: ContinuationCoordinator::new(Box::new(DefaultContinuationPolicy), 0.5),
            #[cfg(feature = "sub-agents")]
            subtask_request_queue: Arc::new(Mutex::new(Vec::new())),
            #[cfg(feature = "sub-agents")]
            subtask_notifications: HashMap::new(),
        }
    }

    #[must_use]
    pub fn next_task_id(&mut self) -> String {
        let id = format!("task-{}", self.task_counter);
        self.task_counter += 1;
        id
    }

    #[must_use]
    pub fn has_conflict(&self, files: &[PathBuf]) -> bool {
        self.tasks.has_conflict_with(files)
    }

    #[must_use]
    pub fn is_dispatched(&self, task_id: &str) -> bool {
        self.tasks.get(task_id).is_some_and(|e| e.dispatched)
    }

    #[must_use]
    pub fn task_focus(&self, task_id: &str) -> Option<&FocusContract> {
        self.tasks.get(task_id).map(|e| &e.focus)
    }

    pub fn latest_heartbeat(&self, task_id: &str) -> Option<&Heartbeat> {
        self.tasks.latest_heartbeat(task_id)
    }

    pub fn task_highlights(&self, task_id: &str) -> Option<&[Highlight]> {
        self.tasks.get(task_id).map(|e| &e.highlights[..])
    }

    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn prefetch_cache(&self) -> Vec<OrchestratorEvent> {
        self.prefetch_cache.lock().expect("poisoned").clone()
    }

    /// Returns the provider Arc so callers (e.g. the completion pipeline's
    /// review stage) can construct an `AuditAgent` without an extra provider
    /// parameter on every call site.
    #[must_use]
    pub fn provider(&self) -> Arc<dyn Provider> {
        self.provider.clone()
    }

    pub fn heartbeat_sender(&self) -> UnboundedSender<(String, Heartbeat)> {
        self.heartbeat_tx.clone()
    }

    pub fn highlight_sender(&self) -> UnboundedSender<(String, Highlight)> {
        self.highlight_tx.clone()
    }

    #[allow(clippy::unused_async, clippy::too_many_arguments)]
    pub async fn dispatch_task(
        &mut self,
        description: &str,
        focus: Vec<String>,
        estimated_files: Vec<PathBuf>,
        parent_task_id: Option<String>,
    ) -> Result<String> {
        let task_id = self.next_task_id();
        let focus_contract = FocusContract::new(focus);
        let context_snapshot: ContextSnapshot = Arc::new(Mutex::new(Vec::new()));

        if self.has_conflict(&estimated_files) {
            let (steering_tx, _rx) = mpsc::unbounded_channel::<SteeringMessage>();
            self.tasks.insert(TaskEntry {
                id: task_id.clone(),
                description: description.to_string(),
                status: TaskStatus::Sleeping,
                latest_heartbeat: None,
                highlights: Vec::new(),
                steering_tx,
                estimated_files,
                focus: focus_contract,
                context_snapshot,
                dispatched: false,
                join_handle: None,
                parent_task_id,
                workspace_path: None,
                #[cfg(feature = "sub-agents")]
                pending_subtasks: std::collections::HashSet::new(),
            });
            return Ok(task_id);
        }

        let workspace = self
            .workspace_manager
            .create(&self.project_path, &task_id)?;
        let workspace_path = workspace.path().to_path_buf();
        let ws_path_for_entry = workspace_path.clone();

        let tool_ctx = ToolContext {
            workspace_path,
            fs: self.fs.clone(),
            proc: self.proc.clone(),
            task_id: Some(task_id.clone()),
        };
        let tools = ToolRegistry::new();

        let (mut task, handle) = Task::new(
            task_id.clone(),
            self.provider.clone(),
            workspace,
            focus_contract.clone(),
            tools,
            tool_ctx,
            self.clock.clone(),
        );

        #[cfg(feature = "sub-agents")]
        {
            let notif_queue = crate::sub_agent::dispatch::new_notification_queue();
            self.subtask_notifications
                .insert(task_id.clone(), notif_queue.clone());
            task.set_subtask_notification_queue(notif_queue);
            task.register_dispatch_subtask_tool(self.subtask_request_queue.clone());
        }

        if let Ok(Some(agents_md)) = crate::extensions::context::load_agents_md(&self.project_path)
        {
            task.set_project_context(agents_md);
        }

        let TaskHandle {
            steering_tx,
            heartbeat_rx,
            highlight_rx,
            output_rx,
        } = handle;

        spawn_relay_channels(
            heartbeat_rx,
            highlight_rx,
            output_rx,
            self.heartbeat_tx.clone(),
            self.highlight_tx.clone(),
            self.output_tx.clone(),
            task_id.clone(),
        );

        let input = description.to_string();
        let join_handle: JoinHandle<TaskOutcome> =
            tokio::spawn(async move { task.run(&input).await });

        self.tasks.insert(TaskEntry {
            id: task_id.clone(),
            description: description.to_string(),
            status: TaskStatus::Sleeping,
            latest_heartbeat: None,
            highlights: Vec::new(),
            steering_tx,
            estimated_files,
            focus: focus_contract,
            context_snapshot,
            dispatched: true,
            join_handle: Some(join_handle),
            parent_task_id,
            workspace_path: Some(ws_path_for_entry),
            #[cfg(feature = "sub-agents")]
            pending_subtasks: std::collections::HashSet::new(),
        });

        Ok(task_id)
    }

    /// Returns the shared sub-task request queue so that `DispatchSubtaskTool`
    /// instances (registered on each Task) enqueue into the same collection
    /// the Orchestrator drains.
    #[cfg(feature = "sub-agents")]
    #[must_use]
    pub fn subtask_request_queue(&self) -> Arc<Mutex<Vec<crate::sub_agent::SubTaskRequest>>> {
        self.subtask_request_queue.clone()
    }

    /// Returns `true` if the given task has sub-tasks that haven't
    /// reached a terminal state yet.
    #[cfg(feature = "sub-agents")]
    #[must_use]
    pub fn task_has_pending_subtasks(&self, task_id: &str) -> bool {
        self.tasks
            .get(task_id)
            .is_some_and(|e| !e.pending_subtasks.is_empty())
    }

    /// Drains all pending sub-task requests and dispatches each as a child
    /// Task via `dispatch_task`. For each request:
    /// 1. Dispatches a child Task with `parent_task_id` set.
    /// 2. Ensures a `SubTaskNotificationQueue` exists for the parent.
    /// 3. Records the child ID in the parent's `pending_subtasks` set.
    ///
    /// Returns `(parent_id, child_id)` pairs for all dispatched children.
    #[cfg(feature = "sub-agents")]
    pub fn drain_subtask_requests(&mut self) -> Vec<(String, String)> {
        let requests: Vec<crate::sub_agent::SubTaskRequest> = {
            let mut guard = self
                .subtask_request_queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };

        let mut dispatched = Vec::new();

        for req in requests {
            let parent_id = req.parent_id.clone();
            tracing::info!(
                parent = %parent_id,
                description = %req.description,
                "Orchestrator draining sub-task request",
            );

            let child_result = match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    let fut = self.dispatch_task(
                        &req.description,
                        req.focus.clone(),
                        Vec::new(),
                        Some(parent_id.clone()),
                    );
                    tokio::task::block_in_place(|| handle.block_on(fut))
                }
                Err(e) => Err(anyhow::anyhow!("no tokio runtime: {e}")),
            };

            match child_result {
                Ok(child_id) => {
                    self.subtask_notifications
                        .entry(parent_id.clone())
                        .or_insert_with(crate::sub_agent::dispatch::new_notification_queue);

                    if let Some(entry) = self.tasks.get_mut(&parent_id) {
                        entry.pending_subtasks.insert(child_id.clone());
                    }

                    if let Err(e) = self.inject_message(
                        &parent_id,
                        Message::user(format!(
                            "[Sub-task dispatched: {} (id: {child_id})]",
                            req.description
                        )),
                    ) {
                        tracing::warn!(
                            "failed to inject subtask dispatch notification for {parent_id}: {e}"
                        );
                    }

                    dispatched.push((parent_id, child_id));
                }
                Err(e) => {
                    tracing::warn!(
                        parent = %parent_id,
                        error = %e,
                        "failed to dispatch sub-task",
                    );
                    if let Err(inject_err) = self.inject_message(
                        &parent_id,
                        Message::user(format!(
                            "[Sub-task '{}' failed to dispatch: {e}]",
                            req.description
                        )),
                    ) {
                        tracing::warn!(
                            "failed to inject dispatch error for {parent_id}: {inject_err}"
                        );
                    }
                }
            }
        }

        dispatched
    }

    /// Checks all parent tasks for completed/failed children. For each
    /// terminal child:
    /// 1. Constructs a `SubTaskNotification` and pushes it to the parent's
    ///    notification queue.
    /// 2. Removes the child from the parent's `pending_subtasks`.
    /// 3. If the parent is `Waiting`, injects a steering message to wake it.
    ///
    /// Returns the count of notifications sent.
    #[cfg(feature = "sub-agents")]
    pub fn check_subtask_completions(&mut self) -> usize {
        let parents: Vec<String> = self
            .tasks
            .iter()
            .filter(|(_, e)| !e.pending_subtasks.is_empty())
            .map(|(_, e)| e.id.clone())
            .collect();

        let mut notifications_sent = 0;

        for parent_id in parents {
            let pending: Vec<String> = match self.tasks.get(&parent_id) {
                Some(e) => e.pending_subtasks.iter().cloned().collect(),
                None => continue,
            };

            let mut completed_ids = Vec::new();
            let mut messages_to_inject = Vec::new();

            for child_id in &pending {
                let (notification, inject_text) = match self.tasks.get(child_id) {
                    Some(child) => match child.status {
                        TaskStatus::Delivered | TaskStatus::Archived => {
                            let result = crate::sub_agent::SubTaskResult {
                                task_id: child_id.clone(),
                                summary: child
                                    .latest_heartbeat
                                    .as_ref()
                                    .map_or(String::new(), |h| h.summary.clone()),
                                artifacts: Vec::new(),
                            };
                            let inject =
                                format!("[Sub-task result] {}: {}", child_id, result.summary);
                            let notif =
                                crate::sub_agent::dispatch::SubTaskNotification::Completed {
                                    sub_task_id: child_id.clone(),
                                    result,
                                    verdict: crate::sub_agent::dispatch::SubTaskVerdict::Delivered,
                                };
                            (Some(notif), inject)
                        }
                        TaskStatus::Stuck | TaskStatus::Axed => {
                            let reason = child
                                .latest_heartbeat
                                .as_ref()
                                .map_or_else(|| "task failed".to_string(), |h| h.summary.clone());
                            let inject = format!("[Sub-task error] {child_id}: {reason}");
                            let notif = crate::sub_agent::dispatch::SubTaskNotification::Failed {
                                sub_task_id: child_id.clone(),
                                reason: reason.clone(),
                            };
                            (Some(notif), inject)
                        }
                        _ => continue,
                    },
                    None => continue,
                };

                if let Some(n) = notification {
                    if let Some(queue) = self.subtask_notifications.get(&parent_id) {
                        let mut guard = queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if guard.len() < 10 {
                            guard.push(n);
                        }
                    }
                    notifications_sent += 1;
                    completed_ids.push(child_id.clone());
                    messages_to_inject.push(inject_text);
                }
            }

            if let Some(entry) = self.tasks.get_mut(&parent_id) {
                for id in &completed_ids {
                    entry.pending_subtasks.remove(id);
                }
            }

            let parent_waiting = self
                .tasks
                .get(&parent_id)
                .is_some_and(|e| e.status == TaskStatus::Waiting);

            if parent_waiting {
                for msg_text in messages_to_inject {
                    let msg = Message::user(msg_text);
                    if let Err(e) = self.inject_message(&parent_id, msg) {
                        tracing::warn!("failed to inject subtask completion for {parent_id}: {e}");
                    }
                }
            }
        }

        notifications_sent
    }

    /// Returns IDs and statuses of all tasks whose `parent_task_id`
    /// matches the given parent. Used by the `/subtasks` slash command
    /// (behind the `sub-agents` feature).
    #[must_use]
    pub fn subtasks_of(&self, parent_task_id: &str) -> Vec<SubTaskRecord> {
        self.tasks
            .iter()
            .filter(|(_, e)| e.parent_task_id.as_deref() == Some(parent_task_id))
            .map(|(_, e)| SubTaskRecord {
                id: e.id.clone(),
                description: e.description.clone(),
                status: e.status,
                progress: e.latest_heartbeat.as_ref().map_or(0.0, |hb| hb.progress),
                summary: e
                    .latest_heartbeat
                    .as_ref()
                    .map_or(String::new(), |hb| hb.summary.clone()),
            })
            .collect()
    }

    /// Returns all tasks that have a `parent_task_id` set (i.e. all
    /// sub-tasks across all parents).
    #[must_use]
    pub fn list_all_subtasks(&self) -> Vec<SubTaskRecord> {
        self.tasks
            .iter()
            .filter(|(_, e)| e.parent_task_id.is_some())
            .map(|(_, e)| SubTaskRecord {
                id: e.id.clone(),
                description: e.description.clone(),
                status: e.status,
                progress: e.latest_heartbeat.as_ref().map_or(0.0, |hb| hb.progress),
                summary: e
                    .latest_heartbeat
                    .as_ref()
                    .map_or(String::new(), |hb| hb.summary.clone()),
            })
            .collect()
    }

    pub fn drain_heartbeats(&mut self) {
        while let Ok((task_id, hb)) = self.heartbeat_rx.try_recv() {
            let event = OrchestratorEvent::new(
                EventKind::Heartbeat,
                task_id.clone(),
                format!("[{:.0}%] {}", hb.progress * 100.0, hb.summary),
            );
            self.memory.push(event);
            self.tasks.record_heartbeat(&task_id, hb);
        }
    }

    pub fn drain_highlights(&mut self) {
        while let Ok((task_id, hl)) = self.highlight_rx.try_recv() {
            let event = OrchestratorEvent::new(
                EventKind::Highlight {
                    task_completed: matches!(
                        self.tasks.get(&task_id).map(|e| e.status),
                        Some(TaskStatus::Delivered | TaskStatus::Archived)
                    ),
                },
                task_id.clone(),
                format!("[{}] {}", hl.tag, hl.summary),
            );
            self.memory.push(event);
            self.tasks.record_highlight(&task_id, hl);
        }
    }

    pub fn drain_outputs(&mut self) -> Vec<(String, crate::task::TaskOutput)> {
        let mut out = Vec::new();
        while let Ok(item) = self.output_rx.try_recv() {
            out.push(item);
        }
        out
    }

    pub fn deep_dive(&self, task_id: &str, query: &str) -> Result<Vec<Message>> {
        let entry = self
            .tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("task not found: {task_id}"))?;
        let snapshot = entry.context_snapshot.lock().expect("poisoned");
        let keywords = extract_keywords(query);
        if keywords.is_empty() {
            return Ok(snapshot.clone());
        }
        Ok(snapshot
            .iter()
            .filter(|msg| {
                let text = msg.text().to_ascii_lowercase();
                keywords.iter().any(|kw| text.contains(kw))
            })
            .cloned()
            .collect())
    }

    pub fn set_context_snapshot(&self, task_id: &str, messages: Vec<Message>) -> Result<()> {
        let entry = self
            .tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("task not found: {task_id}"))?;
        *entry.context_snapshot.lock().expect("poisoned") = messages;
        Ok(())
    }

    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<OrchestratorEvent>> {
        self.memory.recall(query)
    }

    pub fn remember(&self, event: &OrchestratorEvent, tags: &[&str]) -> Result<i64> {
        use crate::memory::{MemoryItem, MemoryMeta};
        let meta = MemoryMeta {
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            searchable_text: event.to_text(),
            timestamp: Some(self.clock.now()),
            task_id: Some(event.task_id.clone()),
        };
        self.memory.remember_with(event, &meta)
    }

    pub fn on_user_message(&mut self, message: &str) {
        let event = OrchestratorEvent::new(
            EventKind::Other,
            "__orchestrator__".to_string(),
            format!("user: {message}"),
        );
        self.memory.push(event);
        self.prefetch(message);
    }

    fn prefetch(&self, keywords: &str) {
        if let Ok(results) = self
            .memory
            .recall(&RecallQuery::Keyword(keywords.to_string()))
        {
            let mut cache = self.prefetch_cache.lock().expect("poisoned");
            cache.clear();
            cache.extend(results);
        }
    }

    pub fn update_focus(&self, task_id: &str, update: FocusUpdate) -> Result<()> {
        let entry = self
            .tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("task not found: {task_id}"))?;
        entry
            .steering_tx
            .send(SteeringMessage::UpdateFocus(update))
            .map_err(|_| anyhow::anyhow!("steering channel closed for task {task_id}"))?;
        Ok(())
    }

    pub fn inject_message(&self, task_id: &str, message: Message) -> Result<()> {
        let entry = self
            .tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("task not found: {task_id}"))?;
        entry
            .steering_tx
            .send(SteeringMessage::Inject(message))
            .map_err(|_| anyhow::anyhow!("steering channel closed for task {task_id}"))?;
        Ok(())
    }

    pub fn cancel_task(&self, task_id: &str) -> Result<()> {
        let entry = self
            .tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("task not found: {task_id}"))?;
        entry
            .steering_tx
            .send(SteeringMessage::Cancel)
            .map_err(|_| anyhow::anyhow!("steering channel closed for task {task_id}"))?;
        Ok(())
    }

    #[must_use]
    pub const fn continuation(&self) -> &ContinuationCoordinator {
        &self.continuation
    }

    pub const fn continuation_mut(&mut self) -> &mut ContinuationCoordinator {
        &mut self.continuation
    }

    /// Evaluates a completed task for continuation. Called after the completion
    /// pipeline returns. Returns a [`DispatchRequest`](crate::continuation::DispatchRequest)
    /// if a continuation iteration should be dispatched.
    pub fn evaluate_continuation(
        &mut self,
        task_id: &str,
        audit_verdict: Option<crate::audit::AuditVerdict>,
        audit_confidence: f64,
        audit_findings: &[crate::audit::Finding],
    ) -> Option<crate::continuation::DispatchRequest> {
        self.continuation.evaluate(
            task_id,
            audit_verdict.as_ref(),
            audit_confidence,
            audit_findings,
        )
    }

    /// Runs an audit on a completed task and returns the verdict, confidence,
    /// and findings. Returns `None` if the task doesn't exist or has no
    /// workspace path (e.g. it was never dispatched).
    pub async fn audit_completed_task(
        &self,
        task_id: &str,
    ) -> Option<(crate::audit::AuditVerdict, f64, Vec<crate::audit::Finding>)> {
        let entry = self.tasks.get(task_id)?;
        let ws_path = entry.workspace_path.as_ref()?;
        let provider = self.provider.clone();
        let task_memory = entry.context_snapshot.clone();

        let agent = crate::audit::AuditAgent::new(
            provider,
            task_id,
            ws_path.clone(),
            crate::workspace::ChangeSet::default(),
            task_memory,
            Vec::new(),
            crate::audit::ModelTier::Cheap,
        );

        match agent.audit().await {
            Ok(report) => Some((report.verdict, report.confidence, report.findings)),
            Err(e) => {
                tracing::warn!("Audit failed for task {task_id}: {e}");
                None
            }
        }
    }
}

/// Spawns three tokio tasks that relay heartbeats, highlights, and outputs
/// from a Task's channels into the Orchestrator's aggregation channels.
fn spawn_relay_channels(
    mut heartbeat_rx: UnboundedReceiver<Heartbeat>,
    mut highlight_rx: UnboundedReceiver<Highlight>,
    mut output_rx: UnboundedReceiver<crate::task::TaskOutput>,
    heartbeat_tx: UnboundedSender<(String, Heartbeat)>,
    highlight_tx: UnboundedSender<(String, Highlight)>,
    output_tx: UnboundedSender<(String, crate::task::TaskOutput)>,
    task_id: String,
) {
    tokio::spawn(async move {
        while let Some(hb) = heartbeat_rx.recv().await {
            let id = hb.task_id.clone();
            let _ = heartbeat_tx.send((id, hb));
        }
    });
    let hl_task_id = task_id.clone();
    tokio::spawn(async move {
        while let Some(hl) = highlight_rx.recv().await {
            let _ = highlight_tx.send((hl_task_id.clone(), hl));
        }
    });
    let out_task_id = task_id;
    tokio::spawn(async move {
        while let Some(out) = output_rx.recv().await {
            let _ = output_tx.send((out_task_id.clone(), out));
        }
    });
}

#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("tasks", &self.tasks.len())
            .field("task_counter", &self.task_counter)
            .field("project_path", &self.project_path)
            .field("prefetch_cache_len", &{
                let cache = self.prefetch_cache.lock().expect("poisoned");
                cache.len()
            })
            .field("active_chains", &{
                let c = &self.continuation;
                c.list_active_chains().len()
            })
            .finish()
    }
}
