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

use super::registry::{ContextSnapshot, TaskEntry, TaskRegistry};

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
        _parent_task_id: Option<String>,
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
                parent_task_id: None,
            });
            return Ok(task_id);
        }

        let workspace = self
            .workspace_manager
            .create(&self.project_path, &task_id)?;
        let workspace_path = workspace.path().to_path_buf();

        let tool_ctx = ToolContext {
            workspace_path,
            fs: self.fs.clone(),
            proc: self.proc.clone(),
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

        let TaskHandle {
            steering_tx,
            heartbeat_rx: mut task_hb_rx,
            highlight_rx: mut task_hl_rx,
            output_rx: mut task_out_rx,
        } = handle;

        let hb_tx = self.heartbeat_tx.clone();
        let hl_tx = self.highlight_tx.clone();
        let highlight_task_id = task_id.clone();

        tokio::spawn(async move {
            while let Some(hb) = task_hb_rx.recv().await {
                let id = hb.task_id.clone();
                let _ = hb_tx.send((id, hb));
            }
        });
        tokio::spawn(async move {
            while let Some(hl) = task_hl_rx.recv().await {
                let _ = hl_tx.send((highlight_task_id.clone(), hl));
            }
        });
        let out_task_id = task_id.clone();
        let out_tx = self.output_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = task_out_rx.recv().await {
                let _ = out_tx.send((out_task_id.clone(), out));
            }
        });

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
            parent_task_id: None,
        });

        Ok(task_id)
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
                let text = msg.content.to_ascii_lowercase();
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
