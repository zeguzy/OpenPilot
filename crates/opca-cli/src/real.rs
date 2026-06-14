use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opca_core::di::{Clock, FileSystem, Process, StdClock, StdFileSystem, StdProcess};
use opca_core::lifecycle::TaskStatus;
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::Provider;
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
}

struct TrackedTask {
    id: String,
    description: String,
    accepted: bool,
    rejected: bool,
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
        let mut subs = subscribers.lock().expect("poisoned");
        for change in changes {
            subs.retain(|tx| tx.send(change.clone()).is_ok());
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
        if current != previous {
            tracked.last_status.insert(task.id.clone(), current);
            if current == TaskStatus::Delivered {
                let summary = hb.map_or("", |h| h.summary.as_str());
                out.push(Notification::Completed {
                    task_id: task.id.clone(),
                    description: task.description.clone(),
                    files_modified: count_files_from_summary(summary),
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
    let fut = orch.dispatch_task(description, Vec::new(), Vec::new());
    tokio::task::block_in_place(|| handle.block_on(fut)).map_err(|e| e.to_string())
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
                warn!("dispatch failed: {e}");
                "dispatch-error".to_string()
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
        let inner = self.inner.clone();
        let tracked = self.tracked.clone();
        let handle = tokio::runtime::Handle::try_current().ok();
        tokio::spawn(async move {
            let result = provider.stream(&[msg], &[], Some(&prompt)).await;
            match result {
                Ok(stream) => {
                    let mut stream = Box::pin(stream);
                    let mut full_text = String::new();
                    let mut line_buffer = String::new();
                    let mut first_line_checked = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(ProviderEvent::TextDelta(delta)) => {
                                full_text.push_str(&delta);
                                if first_line_checked {
                                    let _ = tx.send(crate::tui::app::StreamEvent::Delta(delta));
                                } else {
                                    line_buffer.push_str(&delta);
                                    if line_buffer.contains('\n') {
                                        first_line_checked = true;
                                        if !line_buffer.trim_start().starts_with("[OPCA_DISPATCH]")
                                        {
                                            let _ = tx.send(crate::tui::app::StreamEvent::Delta(
                                                std::mem::take(&mut line_buffer),
                                            ));
                                        }
                                    }
                                }
                            }
                            Ok(ProviderEvent::Done { .. }) => {
                                if !first_line_checked
                                    && !line_buffer.is_empty()
                                    && !line_buffer.trim_start().starts_with("[OPCA_DISPATCH]")
                                {
                                    let _ = tx.send(crate::tui::app::StreamEvent::Delta(
                                        std::mem::take(&mut line_buffer),
                                    ));
                                }
                                break;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                let _ = tx.send(crate::tui::app::StreamEvent::Error(e.to_string()));
                                return;
                            }
                        }
                    }
                    if full_text.trim_start().starts_with("[OPCA_DISPATCH]") {
                        let description = full_text
                            .trim_start()
                            .trim_start_matches("[OPCA_DISPATCH]")
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if description.is_empty() {
                            let _ = tx.send(crate::tui::app::StreamEvent::Done);
                            return;
                        }
                        let _ =
                            tx.send(crate::tui::app::StreamEvent::Dispatch(description.clone()));
                        if let Some(h) = handle {
                            let desc = description;
                            let h2 = h.clone();
                            h.spawn(async move {
                                let result = tokio::task::block_in_place(|| {
                                    h2.block_on(async {
                                        let mut orch = inner.lock().expect("poisoned");
                                        orch.dispatch_task(&desc, Vec::new(), Vec::new()).await
                                    })
                                });
                                if let Ok(id) = result {
                                    tracked.lock().expect("poisoned").tasks.push(TrackedTask {
                                        id,
                                        description: desc,
                                        accepted: false,
                                        rejected: false,
                                    });
                                }
                            });
                        }
                    } else {
                        let _ = tx.send(crate::tui::app::StreamEvent::Done);
                    }
                }
                Err(e) => {
                    let _ = tx.send(crate::tui::app::StreamEvent::Error(e.to_string()));
                }
            }
        });
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
