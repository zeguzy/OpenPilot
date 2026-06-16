use std::collections::HashMap;
use std::sync::Mutex;

use opca_core::lifecycle::TaskStatus;
use opca_core::orchestrator::{RouteDecision, route};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{Notification, OrchestratorApi, Reply, TaskInfo};

#[derive(Debug, Clone)]
struct MockTask {
    info: TaskInfo,
    accepted: bool,
    rejected: bool,
}

#[derive(Debug, Clone)]
struct MockChain {
    display_id: String,
    root_task_id: String,
    prompt: String,
    max_iterations: u32,
    budget: f64,
    active: bool,
    iterations: u32,
}

#[cfg(feature = "sub-agents")]
#[derive(Debug, Clone)]
struct MockSubTask {
    id: String,
    description: String,
    status: TaskStatus,
    progress: f64,
    summary: String,
}

#[derive(Default)]
struct MockState {
    tasks: HashMap<String, MockTask>,
    counter: u64,
    subscribers: Vec<UnboundedSender<Notification>>,
    last_message: Option<String>,
    chains: HashMap<String, MockChain>,
    chain_counter: u64,
    #[cfg(feature = "sub-agents")]
    subtasks: HashMap<String, Vec<MockSubTask>>,
}

#[derive(Default)]
pub struct MockOrchestrator {
    state: Mutex<MockState>,
}

impl MockOrchestrator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(&self) -> String {
        let mut state = self.state.lock().expect("poisoned");
        let id = format!("task-{}", state.counter);
        state.counter += 1;
        id
    }

    pub fn push_heartbeat(&self, task_id: &str, status: TaskStatus, progress: f64, summary: &str) {
        let mut state = self.state.lock().expect("poisoned");
        if let Some(task) = state.tasks.get_mut(task_id) {
            task.info.status = status;
            task.info.progress = progress;
            task.info.summary = summary.to_string();
            let notif = Notification::StatusChanged {
                task_id: task_id.to_string(),
                status,
                summary: summary.to_string(),
            };
            broadcast(&mut state, notif);
        }
    }

    pub fn complete_task(&self, task_id: &str, files_modified: usize) {
        let mut state = self.state.lock().expect("poisoned");
        if let Some(task) = state.tasks.get_mut(task_id) {
            task.info.status = TaskStatus::Delivered;
            task.info.progress = 1.0;
            task.info.files_modified = files_modified;
            let description = task.info.description.clone();
            let notif = Notification::Completed {
                task_id: task_id.to_string(),
                description,
                files_modified,
                summary: String::new(),
            };
            broadcast(&mut state, notif);
        }
    }

    pub fn seed_task(&self, info: TaskInfo) {
        let mut state = self.state.lock().expect("poisoned");
        state.tasks.insert(
            info.id.clone(),
            MockTask {
                info,
                accepted: false,
                rejected: false,
            },
        );
    }

    #[must_use]
    pub fn was_accepted(&self, task_id: &str) -> bool {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .get(task_id)
            .is_some_and(|t| t.accepted)
    }

    #[must_use]
    pub fn was_rejected(&self, task_id: &str) -> bool {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .get(task_id)
            .is_some_and(|t| t.rejected)
    }

    #[must_use]
    pub fn reject_feedback(&self, task_id: &str) -> Option<String> {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .get(task_id)
            .and_then(|t| t.info.summary.clone().into())
    }

    #[must_use]
    pub fn last_message(&self) -> Option<String> {
        self.state.lock().expect("poisoned").last_message.clone()
    }

    #[must_use]
    pub fn task_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .keys()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn chain_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("poisoned")
            .chains
            .keys()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn is_chain_active(&self, chain_id: &str) -> bool {
        self.state
            .lock()
            .expect("poisoned")
            .chains
            .get(chain_id)
            .is_some_and(|c| c.active)
    }

    /// Seeds a sub-task for testing (behind the `sub-agents` feature).
    #[cfg(feature = "sub-agents")]
    pub fn seed_subtask(
        &self,
        parent_id: &str,
        sub_id: &str,
        description: &str,
        status: TaskStatus,
        progress: f64,
    ) {
        let mut state = self.state.lock().expect("poisoned");
        state
            .subtasks
            .entry(parent_id.to_string())
            .or_default()
            .push(MockSubTask {
                id: sub_id.to_string(),
                description: description.to_string(),
                status,
                progress,
                summary: format!("{status}"),
            });
    }
}

fn broadcast(state: &mut MockState, notif: Notification) {
    state
        .subscribers
        .retain(|tx| tx.send(notif.clone()).is_ok());
}

impl OrchestratorApi for MockOrchestrator {
    fn handle_message(&self, message: &str) -> Reply {
        let lower = message.to_ascii_lowercase();
        if let Some(task_id) =
            extract_task_ref(&lower, &["how is", "how's", "status of", "progress of"])
        {
            if let Some(info) = self.task_status(&task_id) {
                return Reply::Foreground(format_task_status(&info));
            }
            return Reply::Foreground(format!("No task named '{task_id}'."));
        }
        if lower.contains("what's running")
            || lower.contains("what is running")
            || lower.contains("running task")
            || lower.contains("active task")
        {
            return Reply::Foreground(format_task_list(&self.list_tasks()));
        }
        match route(message, "") {
            RouteDecision::Background { .. } => {
                let id = self.dispatch(message);
                Reply::Dispatched {
                    task_id: id,
                    description: message.to_string(),
                }
            }
            RouteDecision::Foreground => {
                self.state.lock().expect("poisoned").last_message = Some(message.to_string());
                Reply::Foreground(format!("Echo: {message}"))
            }
        }
    }

    fn dispatch(&self, description: &str) -> String {
        let id = self.next_id();
        let info = TaskInfo {
            id: id.clone(),
            description: description.to_string(),
            status: TaskStatus::Pondering,
            progress: 0.0,
            summary: "analyzing".to_string(),
            files_modified: 0,
        };
        self.seed_task(info);
        id
    }

    fn list_tasks(&self) -> Vec<TaskInfo> {
        let state = self.state.lock().expect("poisoned");
        let mut tasks: Vec<TaskInfo> = state.tasks.values().map(|t| t.info.clone()).collect();
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        tasks
    }

    fn task_status(&self, task_id: &str) -> Option<TaskInfo> {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .get(task_id)
            .map(|t| t.info.clone())
    }

    fn accept(&self, task_id: &str) -> Result<(), String> {
        let mut state = self.state.lock().expect("poisoned");
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("task '{task_id}' not found"))?;
        if task.info.status != TaskStatus::Delivered {
            return Err(format!(
                "task '{task_id}' is {} (must be delivered to accept)",
                task.info.status
            ));
        }
        task.accepted = true;
        task.info.status = TaskStatus::Archived;
        Ok(())
    }

    fn reject(&self, task_id: &str, feedback: Option<&str>) -> Result<(), String> {
        let mut state = self.state.lock().expect("poisoned");
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("task '{task_id}' not found"))?;
        if task.info.status != TaskStatus::Delivered {
            return Err(format!(
                "task '{task_id}' is {} (must be delivered to reject)",
                task.info.status
            ));
        }
        task.rejected = true;
        task.info.status = TaskStatus::OnIt;
        if let Some(fb) = feedback {
            task.info.summary = format!("feedback: {fb}");
        }
        Ok(())
    }

    fn pending_review_count(&self) -> usize {
        self.state
            .lock()
            .expect("poisoned")
            .tasks
            .values()
            .filter(|t| t.info.pending_review())
            .count()
    }

    fn answer_task(&self, task_id: &str, choice: &str) -> Result<(), String> {
        let mut state = self.state.lock().expect("poisoned");
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("task '{task_id}' not found"))?;
        if task.info.status != TaskStatus::Waiting {
            return Err(format!(
                "task '{task_id}' is {} (must be Waiting to answer)",
                task.info.status
            ));
        }
        task.info.status = TaskStatus::OnIt;
        task.info.summary = format!("answered: {choice}");
        let notif = Notification::StatusChanged {
            task_id: task_id.to_string(),
            status: TaskStatus::OnIt,
            summary: format!("resumed with answer: {choice}"),
        };
        broadcast(&mut state, notif);
        Ok(())
    }

    fn subscribe(&self) -> UnboundedReceiver<Notification> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.state.lock().expect("poisoned").subscribers.push(tx);
        rx
    }

    fn stream_foreground(
        &self,
        message: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tui::app::StreamEvent>,
    ) {
        let reply = format!("Echo: {message}");
        tokio::spawn(async move {
            for word in reply.split_inclusive(' ') {
                tx.send(crate::tui::app::StreamEvent::Delta(word.to_string()))
                    .ok();
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
            tx.send(crate::tui::app::StreamEvent::Done).ok();
        });
    }

    fn start_continuation(
        &self,
        prompt: &str,
        max_iterations: Option<u32>,
        budget: Option<f64>,
    ) -> String {
        let task_id = self.dispatch(prompt);
        let mut state = self.state.lock().expect("poisoned");
        state.chain_counter += 1;
        let display_id = format!("chain-{}", state.chain_counter);
        let max_iter = max_iterations.unwrap_or(10);
        let bud = budget.unwrap_or(5.0);
        state.chains.insert(
            display_id.clone(),
            MockChain {
                display_id: display_id.clone(),
                root_task_id: task_id,
                prompt: prompt.to_string(),
                max_iterations: max_iter,
                budget: bud,
                active: true,
                iterations: 0,
            },
        );
        display_id
    }

    fn stop_continuation(&self, chain_id: &str) -> Result<usize, String> {
        let mut state = self.state.lock().expect("poisoned");
        if chain_id.eq_ignore_ascii_case("all") {
            let mut count = 0usize;
            for chain in state.chains.values_mut() {
                if chain.active {
                    chain.active = false;
                    count += 1;
                }
            }
            return Ok(count);
        }
        let chain = state
            .chains
            .get_mut(chain_id)
            .ok_or_else(|| format!("chain '{chain_id}' not found"))?;
        if !chain.active {
            return Ok(0);
        }
        chain.active = false;
        Ok(1)
    }

    fn continuation_status(&self, chain_id: Option<&str>) -> String {
        let state = self.state.lock().expect("poisoned");
        if let Some(id) = chain_id {
            return match state.chains.get(id) {
                Some(chain) => format_mock_chain(chain),
                None => format!("No chain named '{id}'."),
            };
        }
        if state.chains.is_empty() {
            return "No continuation chains.".to_string();
        }
        let active_count = state.chains.values().filter(|c| c.active).count();
        let mut out = format!(
            "Continuation Chains ({} active of {} total):\n",
            active_count,
            state.chains.len()
        );
        let mut chains: Vec<&MockChain> = state.chains.values().collect();
        chains.sort_by(|a, b| a.display_id.cmp(&b.display_id));
        for chain in chains {
            out.push_str("  ");
            out.push_str(&format_mock_chain(chain));
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    #[cfg(feature = "sub-agents")]
    fn list_subtasks(&self, parent_task_id: Option<&str>) -> Vec<crate::SubTaskInfo> {
        let state = self.state.lock().expect("poisoned");
        state
            .subtasks
            .iter()
            .filter(|(pid, _)| parent_task_id.is_none_or(|p| pid.as_str() == p))
            .flat_map(|(_, subs)| subs.iter())
            .map(|s| crate::SubTaskInfo {
                id: s.id.clone(),
                description: s.description.clone(),
                status: s.status,
                progress: s.progress,
                summary: s.summary.clone(),
            })
            .collect()
    }
}

fn format_mock_chain(chain: &MockChain) -> String {
    let status = if chain.active { "active" } else { "stopped" };
    format!(
        "{} [{}] iter {}/{}, budget ${:.2}, root {} — {}",
        chain.display_id,
        status,
        chain.iterations,
        chain.max_iterations,
        chain.budget,
        chain.root_task_id,
        chain.prompt,
    )
}

fn extract_task_ref(lower: &str, prefixes: &[&str]) -> Option<String> {
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace());
            let candidate = trimmed.split_whitespace().next()?;
            if candidate.starts_with("task") {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

pub fn format_task_status(info: &TaskInfo) -> String {
    let pct = (info.progress * 100.0).round() as u32;
    format!(
        "{} {} [{} {}%] — {}",
        info.status.emoji(),
        info.id,
        info.status,
        pct,
        info.summary
    )
}

pub fn format_task_list(tasks: &[TaskInfo]) -> String {
    if tasks.is_empty() {
        return "No tasks.".to_string();
    }
    let mut out = String::from("Active Tasks:\n");
    for info in tasks {
        out.push_str("  ");
        out.push_str(&format_task_line(info));
        out.push('\n');
    }
    out.trim_end().to_string()
}

pub fn format_task_line(info: &TaskInfo) -> String {
    let pct = (info.progress * 100.0).round() as u32;
    if info.pending_review() {
        format!(
            "{} {} [{}] — pending review (use /accept or /reject)",
            info.status.emoji(),
            info.id,
            info.status,
        )
    } else {
        format!(
            "{} {} [{} {}%] — {}",
            info.status.emoji(),
            info.id,
            info.status,
            pct,
            info.summary,
        )
    }
}
