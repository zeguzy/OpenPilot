use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use crate::focus::{FocusContract, Highlight};
use crate::lifecycle::{Heartbeat, TaskStatus};
use crate::provider::Message;
use crate::task::{SteeringMessage, TaskOutcome};

pub type ContextSnapshot = Arc<Mutex<Vec<Message>>>;

pub struct TaskEntry {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub latest_heartbeat: Option<Heartbeat>,
    pub highlights: Vec<Highlight>,
    pub steering_tx: UnboundedSender<SteeringMessage>,
    pub estimated_files: Vec<PathBuf>,
    pub focus: FocusContract,
    pub context_snapshot: ContextSnapshot,
    pub dispatched: bool,
    pub join_handle: Option<JoinHandle<TaskOutcome>>,
    pub parent_task_id: Option<String>,
}

impl TaskEntry {
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.dispatched
            && !matches!(
                self.status,
                TaskStatus::Delivered | TaskStatus::Stuck | TaskStatus::Axed | TaskStatus::Archived
            )
    }
}

#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for TaskEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskEntry")
            .field("id", &self.id)
            .field("description", &self.description)
            .field("status", &self.status)
            .field("latest_heartbeat", &self.latest_heartbeat.is_some())
            .field("highlights", &self.highlights.len())
            .field("estimated_files", &self.estimated_files)
            .field("focus", &self.focus)
            .field("dispatched", &self.dispatched)
            .field("join_handle", &self.join_handle.is_some())
            .finish()
    }
}

#[derive(Default)]
pub struct TaskRegistry {
    entries: HashMap<String, TaskEntry>,
}

impl TaskRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: TaskEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&TaskEntry> {
        self.entries.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut TaskEntry> {
        self.entries.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<TaskEntry> {
        self.entries.remove(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &TaskEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut TaskEntry)> {
        self.entries.iter_mut().map(|(k, v)| (k.as_str(), v))
    }

    #[must_use]
    pub fn active_estimated_files(&self) -> Vec<PathBuf> {
        self.entries
            .values()
            .filter(|e| e.is_active())
            .flat_map(|e| e.estimated_files.iter().cloned())
            .collect()
    }

    #[must_use]
    pub fn has_conflict_with(&self, files: &[PathBuf]) -> bool {
        let active = self.active_estimated_files();
        crate::orchestrator::predict_conflict(&active, files)
    }

    pub fn record_heartbeat(&mut self, task_id: &str, heartbeat: Heartbeat) {
        if let Some(entry) = self.entries.get_mut(task_id) {
            entry.status = heartbeat.status;
            entry.latest_heartbeat = Some(heartbeat);
        }
    }

    pub fn record_highlight(&mut self, task_id: &str, highlight: Highlight) {
        if let Some(entry) = self.entries.get_mut(task_id) {
            entry.highlights.push(highlight);
        }
    }

    #[must_use]
    pub fn latest_heartbeat(&self, task_id: &str) -> Option<&Heartbeat> {
        self.entries
            .get(task_id)
            .and_then(|e| e.latest_heartbeat.as_ref())
    }

    #[must_use]
    pub fn highlights(&self, task_id: &str) -> &[Highlight] {
        self.entries.get(task_id).map_or(&[], |e| &e.highlights[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn dummy_entry(id: &str, files: Vec<PathBuf>, dispatched: bool) -> TaskEntry {
        let (tx, _rx) = mpsc::unbounded_channel::<SteeringMessage>();
        TaskEntry {
            id: id.to_string(),
            description: format!("desc-{id}"),
            status: TaskStatus::Sleeping,
            latest_heartbeat: None,
            highlights: Vec::new(),
            steering_tx: tx,
            estimated_files: files,
            focus: FocusContract::empty(),
            context_snapshot: Arc::new(Mutex::new(Vec::new())),
            dispatched,
            join_handle: None,
            parent_task_id: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let mut reg = TaskRegistry::new();
        reg.insert(dummy_entry("t1", vec![], false));
        assert_eq!(reg.len(), 1);
        assert!(reg.get("t1").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn remove_returns_entry() {
        let mut reg = TaskRegistry::new();
        reg.insert(dummy_entry("t1", vec![], false));
        let removed = reg.remove("t1");
        assert!(removed.is_some());
        assert!(reg.is_empty());
    }

    #[test]
    fn conflict_detection_non_dispatched_ignored() {
        let mut reg = TaskRegistry::new();
        reg.insert(dummy_entry("t1", vec![PathBuf::from("src/auth.rs")], false));
        assert!(!reg.has_conflict_with(&[PathBuf::from("src/auth.rs")]));
    }

    #[test]
    fn conflict_detection_active_dispatched() {
        let mut reg = TaskRegistry::new();
        let mut e = dummy_entry("t1", vec![PathBuf::from("src/auth.rs")], true);
        e.status = TaskStatus::OnIt;
        reg.insert(e);
        assert!(reg.has_conflict_with(&[PathBuf::from("src/auth.rs")]));
        assert!(!reg.has_conflict_with(&[PathBuf::from("src/utils.rs")]));
    }

    #[test]
    fn record_heartbeat_updates_status() {
        let mut reg = TaskRegistry::new();
        reg.insert(dummy_entry("t1", vec![], false));
        let hb = Heartbeat {
            task_id: "t1".to_string(),
            status: TaskStatus::OnIt,
            progress: 0.5,
            summary: "working".to_string(),
            timestamp: 1,
        };
        reg.record_heartbeat("t1", hb);
        let entry = reg.get("t1").unwrap();
        assert_eq!(entry.status, TaskStatus::OnIt);
        assert!(entry.latest_heartbeat.is_some());
        assert_eq!(reg.latest_heartbeat("t1").unwrap().summary, "working");
    }
}
