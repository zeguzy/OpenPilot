use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opca_core::continuation::{
    ChainId, ChainStatus, ChainTerminationReason, ContinuationBudget, ContinuationChain,
};
use opca_core::di::{Clock, FileSystem, Process, StdClock, StdFileSystem, StdProcess};
use opca_core::lifecycle::TaskStatus;
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::{Provider, ToolDef, ToolEffects};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::warn;

use crate::{Notification, OrchestratorApi, Reply, TaskInfo};

pub struct RealOrchestrator {
    inner: Arc<Mutex<Orchestrator>>,
    tracked: Arc<Mutex<TrackedTasks>>,
    subscribers: Arc<Mutex<Vec<UnboundedSender<Notification>>>>,
    #[allow(dead_code)]
    project_path: PathBuf,
    #[allow(dead_code)]
    provider: Arc<dyn Provider>,
}

#[derive(Default)]
struct TrackedTasks {
    tasks: Vec<TrackedTask>,
    last_status: HashMap<String, TaskStatus>,
    clarification_sent: std::collections::HashSet<String>,
    waiting_since: HashMap<String, std::time::Instant>,
}

struct TrackedTask {
    id: String,
    description: String,
    accepted: bool,
    rejected: bool,
}

const CLARIFICATION_TIMEOUT_SECS: u64 = 300;
const CLARIFICATION_PREFIX: &str = "Waiting for clarification:";

#[must_use]
fn dispatch_task_tool_def() -> ToolDef {
    ToolDef {
        name: "dispatch_task".to_string(),
        description: "Dispatch a background Task to work on a long-running job. Use this instead \
            of writing plain text when the user's request involves implementation, multi-step \
            work, or background processing."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The full prompt to send to the Task. Should include context, goal, and any constraints."
                },
                "focus_dimensions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional focus dimensions for the Task (e.g., [\"compilation\", \"tests\"]). Defaults to []"
                },
                "predecessors": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional Task IDs that must complete before this Task starts."
                }
            },
            "required": ["prompt"]
        }),
        effects: ToolEffects::Process,
    }
}

impl RealOrchestrator {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn Provider>,
        project_path: PathBuf,
        clock: Arc<dyn Clock>,
        fs: Arc<dyn FileSystem>,
        proc: Arc<dyn Process>,
    ) -> Self {
        let orchestrator =
            Orchestrator::new(provider.clone(), project_path.clone(), clock, fs, proc);
        let inner = Arc::new(Mutex::new(orchestrator));
        let tracked = Arc::new(Mutex::new(TrackedTasks::default()));
        let subscribers: Arc<Mutex<Vec<_>>> = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn(poll_loop(
            inner.clone(),
            tracked.clone(),
            subscribers.clone(),
        ));
        Self {
            inner,
            tracked,
            subscribers,
            project_path,
            provider,
        }
    }

    #[must_use]
    pub fn with_std_di(provider: Arc<dyn Provider>, project_path: PathBuf) -> Self {
        Self::new(
            provider,
            project_path,
            Arc::new(StdClock),
            Arc::new(StdFileSystem),
            Arc::new(StdProcess),
        )
    }

    #[allow(dead_code)]
    fn query_llm(&self, message: &str) -> String {
        use futures::StreamExt;
        use opca_core::provider::{Message, ProviderEvent};

        let provider = self.provider.clone();
        let msg = Message::user(message.to_string());
        let messages = vec![msg];
        let handle = tokio::runtime::Handle::try_current();
        let result = match handle {
            Ok(h) => tokio::task::block_in_place(|| {
                h.block_on(async {
                    let stream = provider
                        .stream(
                            &messages,
                            &[],
                            Some(opca_core::provider::orchestrator_prompt()),
                        )
                        .await?;
                    let mut stream = Box::pin(stream);
                    let mut text = String::new();
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(ProviderEvent::TextDelta(delta)) => text.push_str(&delta),
                            Ok(ProviderEvent::Done { .. } | ProviderEvent::Error(_)) => break,
                            _ => {}
                        }
                    }
                    Ok::<_, anyhow::Error>(text)
                })
            }),
            Err(_) => return "(error: no tokio runtime)".to_string(),
        };
        match result {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => "(empty response from provider)".to_string(),
            Err(e) => format!("(provider error: {e})"),
        }
    }
}

async fn poll_loop(
    inner: Arc<Mutex<Orchestrator>>,
    tracked: Arc<Mutex<TrackedTasks>>,
    subscribers: Arc<Mutex<Vec<UnboundedSender<Notification>>>>,
) {
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    loop {
        ticker.tick().await;
        let changes = {
            let mut orch = inner.lock().expect("poisoned");
            orch.drain_heartbeats();
            orch.drain_highlights();
            let mut t = tracked.lock().expect("poisoned");
            collect_changes(&mut t, &orch)
        };
        {
            let mut subs = subscribers.lock().expect("poisoned");
            for change in changes {
                subs.retain(|tx| tx.send(change.clone()).is_ok());
            }
        }
        check_clarification_timeouts(&inner, &tracked);
        check_continuations(&inner, &tracked).await;
        #[cfg(feature = "sub-agents")]
        {
            let mut orch = inner.lock().expect("poisoned");
            let _ = orch.drain_subtask_requests();
            orch.check_subtask_completions();
        }
    }
}

fn collect_changes(tracked: &mut TrackedTasks, orch: &Orchestrator) -> Vec<Notification> {
    let mut out = Vec::new();
    for task in &tracked.tasks {
        let hb = orch.latest_heartbeat(&task.id);
        let current = hb.map_or(TaskStatus::Sleeping, |h| h.status);
        let previous = tracked
            .last_status
            .get(&task.id)
            .copied()
            .unwrap_or(TaskStatus::Sleeping);

        if current == TaskStatus::Waiting && !tracked.clarification_sent.contains(&task.id) {
            tracked.clarification_sent.insert(task.id.clone());
            tracked
                .waiting_since
                .insert(task.id.clone(), std::time::Instant::now());
            let summary = hb.map_or("", |h| h.summary.as_str());
            let (question, options) = parse_clarification_heartbeat(summary);
            out.push(Notification::Clarification {
                task_id: task.id.clone(),
                question,
                options,
                timeout_secs: CLARIFICATION_TIMEOUT_SECS,
            });
            continue;
        }

        if current != previous {
            tracked.last_status.insert(task.id.clone(), current);
            tracked.clarification_sent.remove(&task.id);
            tracked.waiting_since.remove(&task.id);
            if current == TaskStatus::Delivered {
                let summary = hb.map_or(String::new(), |h| h.summary.clone());
                out.push(Notification::Completed {
                    task_id: task.id.clone(),
                    description: task.description.clone(),
                    files_modified: count_files_from_summary(&summary),
                    summary,
                });
            } else if let Some(h) = hb {
                out.push(Notification::StatusChanged {
                    task_id: task.id.clone(),
                    status: h.status,
                    summary: h.summary.clone(),
                });
            }
        }
    }
    out
}

fn parse_clarification_heartbeat(summary: &str) -> (String, Vec<String>) {
    let question_part = summary
        .strip_prefix(CLARIFICATION_PREFIX)
        .unwrap_or(summary);
    let question = question_part.trim().to_string();
    (question, Vec::new())
}

fn count_files_from_summary(summary: &str) -> usize {
    summary
        .split_whitespace()
        .filter(|s| {
            s.ends_with(".rs") || s.ends_with(".ts") || s.ends_with(".py") || s.ends_with(".md")
        })
        .count()
}

fn block_on_dispatch(
    orch: &mut std::sync::MutexGuard<'_, Orchestrator>,
    description: &str,
) -> Result<String, String> {
    let handle = tokio::runtime::Handle::try_current().map_err(|e| e.to_string())?;
    let fut = orch.dispatch_task(description, Vec::new(), Vec::new(), None);
    tokio::task::block_in_place(|| handle.block_on(fut)).map_err(|e| e.to_string())
}

fn check_clarification_timeouts(
    inner: &Arc<Mutex<Orchestrator>>,
    tracked: &Arc<Mutex<TrackedTasks>>,
) {
    let timed_out: Vec<String> = {
        let orch = inner.lock().expect("poisoned");
        let t = tracked.lock().expect("poisoned");
        let now = std::time::Instant::now();
        t.waiting_since
            .iter()
            .filter_map(|(id, since)| {
                let status = orch
                    .latest_heartbeat(id)
                    .map_or(TaskStatus::Sleeping, |h| h.status);
                if status == TaskStatus::Waiting
                    && now.duration_since(*since).as_secs() >= CLARIFICATION_TIMEOUT_SECS
                {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    };

    for task_id in timed_out {
        let orch = inner.lock().expect("poisoned");
        let msg = opca_core::provider::Message::user(
            "Clarification timed out. Proceeding with best-guess interpretation.".to_string(),
        );
        if let Err(e) = orch.inject_message(&task_id, msg) {
            warn!("clarification timeout inject failed for {task_id}: {e}");
        }
        let mut t = tracked.lock().expect("poisoned");
        t.clarification_sent.remove(&task_id);
        t.waiting_since.remove(&task_id);
    }
}

async fn check_continuations(inner: &Arc<Mutex<Orchestrator>>, tracked: &Arc<Mutex<TrackedTasks>>) {
    let delivered_tasks: Vec<String> = {
        let orch = inner.lock().expect("poisoned");
        orch.continuation()
            .list_active_chains()
            .iter()
            .filter_map(|chain| {
                let tid = chain.current_task_id();
                match orch.latest_heartbeat(tid) {
                    Some(hb) if hb.status == TaskStatus::Delivered => {
                        #[cfg(feature = "sub-agents")]
                        if orch.task_has_pending_subtasks(tid) {
                            return None;
                        }
                        Some(tid.to_string())
                    }
                    _ => None,
                }
            })
            .collect()
    };

    for task_id in delivered_tasks {
        let audit_result = {
            let orch = inner.lock().expect("poisoned");
            match tokio::runtime::Handle::try_current() {
                Ok(h) => {
                    tokio::task::block_in_place(|| h.block_on(orch.audit_completed_task(&task_id)))
                }
                Err(_) => None,
            }
        };

        let (verdict, confidence, findings) =
            audit_result.unwrap_or((opca_core::audit::AuditVerdict::Confirmed, 1.0, Vec::new()));

        let mut orch = inner.lock().expect("poisoned");
        let req = orch.evaluate_continuation(&task_id, Some(verdict), confidence, &findings);
        if let Some(req) = req {
            let chain_id = req.chain_id.clone();
            let parent = req.parent_task_id.clone();
            let prompt = req.prompt_seed.clone();
            let fut = orch.dispatch_task(&prompt, Vec::new(), Vec::new(), Some(parent));
            let handle = tokio::runtime::Handle::try_current();
            if let Ok(h) = handle {
                match tokio::task::block_in_place(|| h.block_on(fut)) {
                    Ok(new_id) => {
                        orch.continuation_mut()
                            .set_current_task(&chain_id, new_id.clone());
                        drop(orch);
                        tracked.lock().expect("poisoned").tasks.push(TrackedTask {
                            id: new_id,
                            description: prompt,
                            accepted: false,
                            rejected: false,
                        });
                        tracing::info!("Continuation dispatched for chain {chain_id}");
                    }
                    Err(e) => tracing::warn!("Continuation dispatch failed: {e}"),
                }
            }
        }
    }
}

fn build_task_info(orch: &Orchestrator, tracked: &TrackedTasks, task_id: &str) -> Option<TaskInfo> {
    let entry = tracked.tasks.iter().find(|t| t.id == task_id)?;
    let hb = orch.latest_heartbeat(task_id);
    let (status, progress, summary) = match hb {
        Some(h) => (h.status, h.progress, h.summary.clone()),
        None => (TaskStatus::Sleeping, 0.0, "queued".to_string()),
    };
    Some(TaskInfo {
        id: entry.id.clone(),
        description: entry.description.clone(),
        status,
        progress,
        summary,
        files_modified: 0,
    })
}

struct PendingToolCall {
    name: String,
    args: String,
}

impl OrchestratorApi for RealOrchestrator {
    fn handle_message(&self, message: &str) -> Reply {
        let lower = message.to_ascii_lowercase();
        if let Some(task_id) = find_task_ref(&lower) {
            if let Some(info) = self.task_status(&task_id) {
                return Reply::Acknowledged(crate::mock::format_task_status(&info));
            }
        }
        if lower.contains("running") || lower.contains("active task") {
            return Reply::Acknowledged(crate::mock::format_task_list(&self.list_tasks()));
        }
        Reply::Foreground(String::new())
    }

    fn dispatch(&self, description: &str) -> String {
        let mut orch = self.inner.lock().expect("poisoned");
        let result = block_on_dispatch(&mut orch, description);
        match result {
            Ok(id) => {
                self.tracked
                    .lock()
                    .expect("poisoned")
                    .tasks
                    .push(TrackedTask {
                        id: id.clone(),
                        description: description.to_string(),
                        accepted: false,
                        rejected: false,
                    });
                id
            }
            Err(e) => {
                tracing::error!("dispatch failed: {e}");
                format!("dispatch-error: {e}")
            }
        }
    }

    fn list_tasks(&self) -> Vec<TaskInfo> {
        let orch = self.inner.lock().expect("poisoned");
        let tracked = self.tracked.lock().expect("poisoned");
        tracked
            .tasks
            .iter()
            .filter_map(|t| build_task_info(&orch, &tracked, &t.id))
            .collect()
    }

    fn task_status(&self, task_id: &str) -> Option<TaskInfo> {
        let orch = self.inner.lock().expect("poisoned");
        let tracked = self.tracked.lock().expect("poisoned");
        build_task_info(&orch, &tracked, task_id)
    }

    fn accept(&self, task_id: &str) -> Result<(), String> {
        let orch = self.inner.lock().expect("poisoned");
        if !orch.is_dispatched(task_id) {
            return Err(format!("task '{task_id}' not found"));
        }
        let status = orch
            .latest_heartbeat(task_id)
            .map_or(TaskStatus::Sleeping, |h| h.status);
        if status != TaskStatus::Delivered {
            return Err(format!(
                "task '{task_id}' is {status}, must be delivered to accept"
            ));
        }
        drop(orch);
        if let Some(t) = self
            .tracked
            .lock()
            .expect("poisoned")
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
        {
            t.accepted = true;
        }
        Ok(())
    }

    fn reject(&self, task_id: &str, feedback: Option<&str>) -> Result<(), String> {
        let orch = self.inner.lock().expect("poisoned");
        if !orch.is_dispatched(task_id) {
            return Err(format!("task '{task_id}' not found"));
        }
        let status = orch
            .latest_heartbeat(task_id)
            .map_or(TaskStatus::Sleeping, |h| h.status);
        if status != TaskStatus::Delivered {
            return Err(format!(
                "task '{task_id}' is {status}, must be delivered to reject"
            ));
        }
        if let Some(fb) = feedback {
            let msg = opca_core::provider::Message::user(fb.to_string());
            if let Err(e) = orch.inject_message(task_id, msg) {
                warn!("inject feedback failed: {e}");
            }
        }
        drop(orch);
        if let Some(t) = self
            .tracked
            .lock()
            .expect("poisoned")
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
        {
            t.rejected = true;
        }
        Ok(())
    }

    fn pending_review_count(&self) -> usize {
        self.list_tasks()
            .iter()
            .filter(|t| t.pending_review())
            .count()
    }

    fn answer_task(&self, task_id: &str, choice: &str) -> Result<(), String> {
        let orch = self.inner.lock().expect("poisoned");
        if !orch.is_dispatched(task_id) {
            return Err(format!("task '{task_id}' not found"));
        }
        let status = orch
            .latest_heartbeat(task_id)
            .map_or(TaskStatus::Sleeping, |h| h.status);
        if status != TaskStatus::Waiting {
            return Err(format!(
                "task '{task_id}' is {status}, must be Waiting to answer"
            ));
        }
        let msg = opca_core::provider::Message::user(format!("User answered: {choice}"));
        if let Err(e) = orch.inject_message(task_id, msg) {
            warn!("inject answer failed: {e}");
            return Err(e.to_string());
        }
        Ok(())
    }

    fn subscribe(&self) -> UnboundedReceiver<Notification> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscribers.lock().expect("poisoned").push(tx);
        rx
    }

    fn stream_foreground(
        &self,
        message: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tui::app::StreamEvent>,
    ) {
        use futures::StreamExt;
        use opca_core::provider::{Message, ProviderEvent};

        let provider = self.provider.clone();
        let msg = Message::user(message.to_string());
        let prompt = opca_core::provider::orchestrator_prompt().to_string();
        let tools = vec![dispatch_task_tool_def()];
        tokio::spawn(async move {
            let result = provider.stream(&[msg], &tools, Some(&prompt)).await;
            match result {
                Ok(stream) => {
                    let mut stream = Box::pin(stream);
                    let mut pending_tool_calls: HashMap<String, PendingToolCall> = HashMap::new();
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(ProviderEvent::TextDelta(delta)) => {
                                let _ = tx.send(crate::tui::app::StreamEvent::Delta(delta));
                            }
                            Ok(ProviderEvent::ThinkingDelta(delta)) => {
                                let _ = tx.send(crate::tui::app::StreamEvent::Thinking(delta));
                            }
                            Ok(ProviderEvent::ToolCallStart { id, name }) => {
                                pending_tool_calls.insert(
                                    id,
                                    PendingToolCall {
                                        name,
                                        args: String::new(),
                                    },
                                );
                            }
                            Ok(ProviderEvent::ToolCallArgs { id, args }) => {
                                if let Some(tc) = pending_tool_calls.get_mut(&id) {
                                    tc.args.push_str(&args);
                                }
                            }
                            Ok(ProviderEvent::ToolCallEnd { id }) => {
                                if let Some(tc) = pending_tool_calls.remove(&id) {
                                    if tc.name == "dispatch_task" {
                                        match parse_dispatch_args(&tc.args) {
                                            Ok(prompt) => {
                                                let _ = tx.send(
                                                    crate::tui::app::StreamEvent::Dispatch(prompt),
                                                );
                                            }
                                            Err(e) => {
                                                let _ =
                                                    tx.send(crate::tui::app::StreamEvent::Error(
                                                        format!("dispatch_task parse error: {e}"),
                                                    ));
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(ProviderEvent::Done { .. }) => {
                                let _ = tx.send(crate::tui::app::StreamEvent::Done);
                                break;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                let _ = tx.send(crate::tui::app::StreamEvent::Error(e.to_string()));
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(crate::tui::app::StreamEvent::Error(e.to_string()));
                }
            }
        });
    }

    fn start_continuation(
        &self,
        prompt: &str,
        max_iterations: Option<u32>,
        budget: Option<f64>,
    ) -> String {
        let task_id = {
            let mut orch = self.inner.lock().expect("poisoned");
            match block_on_dispatch(&mut orch, prompt) {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!("continuation dispatch failed: {e}");
                    return format!("dispatch-error: {e}");
                }
            }
        };
        self.tracked
            .lock()
            .expect("poisoned")
            .tasks
            .push(TrackedTask {
                id: task_id.clone(),
                description: prompt.to_string(),
                accepted: false,
                rejected: false,
            });

        let budget = build_budget(max_iterations, budget);
        let chain_id = {
            let mut orch = self.inner.lock().expect("poisoned");
            orch.continuation_mut().start_chain(task_id.clone(), budget)
        };
        tracing::info!(
            "started continuation chain {} rooted at task {task_id}",
            chain_id.as_str()
        );
        chain_id.as_str().to_string()
    }

    fn stop_continuation(&self, chain_id: &str) -> Result<usize, String> {
        let mut orch = self.inner.lock().expect("poisoned");
        if chain_id.eq_ignore_ascii_case("all") {
            let active_ids: Vec<ChainId> = orch
                .continuation()
                .list_active_chains()
                .iter()
                .map(|c| c.id().clone())
                .collect();
            let count = active_ids.len();
            for id in active_ids {
                orch.continuation_mut()
                    .terminate(&id, ChainTerminationReason::UserCancelled);
            }
            return Ok(count);
        }
        let chain = orch
            .continuation()
            .list_active_chains()
            .iter()
            .find(|c| c.id().as_str() == chain_id)
            .map(|c| c.id().clone())
            .ok_or_else(|| format!("chain '{chain_id}' not found or already terminated"))?;
        orch.continuation_mut()
            .terminate(&chain, ChainTerminationReason::UserCancelled);
        Ok(1)
    }

    fn continuation_status(&self, chain_id: Option<&str>) -> String {
        let orch = self.inner.lock().expect("poisoned");
        if let Some(id) = chain_id {
            let chain = find_chain_by_str(orch.continuation().list_active_chains(), id);
            return match chain {
                Some(c) => format_chain_detail(c),
                None => format!("No active chain named '{id}'."),
            };
        }
        let active = orch.continuation().list_active_chains();
        if active.is_empty() {
            return "No active continuation chains.".to_string();
        }
        let mut out = format!("Active Continuation Chains ({}):\n", active.len());
        for chain in active {
            out.push_str("  ");
            out.push_str(&format_chain_detail(chain));
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    #[cfg(feature = "sub-agents")]
    fn list_subtasks(&self, parent_task_id: Option<&str>) -> Vec<crate::SubTaskInfo> {
        let orch = self.inner.lock().expect("poisoned");
        if let Some(pid) = parent_task_id {
            orch.subtasks_of(pid)
                .into_iter()
                .map(|r| crate::SubTaskInfo {
                    id: r.id,
                    description: r.description,
                    status: r.status,
                    progress: r.progress,
                    summary: r.summary,
                })
                .collect()
        } else {
            orch.list_all_subtasks()
                .into_iter()
                .map(|r| crate::SubTaskInfo {
                    id: r.id,
                    description: r.description,
                    status: r.status,
                    progress: r.progress,
                    summary: r.summary,
                })
                .collect()
        }
    }
}

fn parse_dispatch_args(args_json: &str) -> Result<String, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("invalid JSON: {e}"))?;
    parsed
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "missing required 'prompt' field".to_string())
}

fn find_chain_by_str<'a>(
    chains: Vec<&'a ContinuationChain>,
    needle: &str,
) -> Option<&'a ContinuationChain> {
    chains.into_iter().find(|c| c.id().as_str() == needle)
}

const DEFAULT_MAX_ITERATIONS: u32 = 10;
const DEFAULT_MAX_TOTAL_COST_USD: f64 = 5.0;
const DEFAULT_MAX_TOTAL_DURATION: Duration = Duration::from_secs(30 * 60);
const DEFAULT_MAX_NO_PROGRESS_ROUNDS: u32 = 2;

fn build_budget(max_iterations: Option<u32>, budget: Option<f64>) -> ContinuationBudget {
    ContinuationBudget::new(
        max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS),
        budget.unwrap_or(DEFAULT_MAX_TOTAL_COST_USD),
        DEFAULT_MAX_TOTAL_DURATION,
        DEFAULT_MAX_NO_PROGRESS_ROUNDS,
    )
}

fn format_chain_detail(chain: &ContinuationChain) -> String {
    let id = chain.id().as_str();
    let iter = chain.current_iteration();
    let max_iter = chain.budget().max_iterations();
    let cost = chain.budget().accumulated_cost_usd();
    let max_cost = chain.budget().max_total_cost_usd();
    let elapsed = chain.budget().elapsed();
    let status = match chain.status() {
        ChainStatus::Active => "active".to_string(),
        ChainStatus::Terminated(reason) => {
            format!("terminated ({})", format_termination_reason(reason))
        }
    };
    format!(
        "{id} [{status}] iteration {iter}/{max_iter}, ${cost:.2}/${max_cost:.2}, {elapsed:?} elapsed — root {}",
        chain.root_task_id(),
    )
}

const fn format_termination_reason(reason: &ChainTerminationReason) -> &'static str {
    match reason {
        ChainTerminationReason::ConfirmedComplete => "confirmed complete",
        ChainTerminationReason::BudgetExhausted(_) => "budget exhausted",
        ChainTerminationReason::NoProgress => "no progress",
        ChainTerminationReason::UserCancelled => "user cancelled",
        ChainTerminationReason::NeedsHumanReview => "needs human review",
        ChainTerminationReason::TaskError(_) => "task error",
    }
}

fn find_task_ref(lower: &str) -> Option<String> {
    for marker in &["how is ", "how's ", "status of ", "progress of "] {
        if let Some(rest) = lower.strip_prefix(marker) {
            let candidate = rest.split_whitespace().next()?;
            if candidate.starts_with("task") {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dispatch_args_extracts_prompt() {
        let args = r#"{"prompt": "refactor the auth module"}"#;
        assert_eq!(
            parse_dispatch_args(args).unwrap(),
            "refactor the auth module"
        );
    }

    #[test]
    fn parse_dispatch_args_with_focus_and_predecessors() {
        let args = r#"{"prompt": "implement JWT", "focus_dimensions": ["tests"], "predecessors": ["task-0"]}"#;
        assert_eq!(parse_dispatch_args(args).unwrap(), "implement JWT");
    }

    #[test]
    fn parse_dispatch_args_missing_prompt_errors() {
        let args = r#"{"focus_dimensions": ["tests"]}"#;
        assert!(parse_dispatch_args(args).is_err());
    }

    #[test]
    fn parse_dispatch_args_invalid_json_errors() {
        let args = "not json";
        assert!(parse_dispatch_args(args).is_err());
    }

    #[test]
    fn dispatch_task_tool_def_has_correct_name() {
        let def = dispatch_task_tool_def();
        assert_eq!(def.name, "dispatch_task");
        assert_eq!(def.effects, ToolEffects::Process);
    }

    #[test]
    fn dispatch_task_tool_def_requires_prompt() {
        let def = dispatch_task_tool_def();
        let required = def
            .parameters
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required field exists");
        let prompt_required = required.iter().any(|v| v.as_str() == Some("prompt"));
        assert!(prompt_required);
    }
}
