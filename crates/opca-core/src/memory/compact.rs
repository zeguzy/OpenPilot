//! Compaction strategies — decide *what* to move out of the active region
//! and *how* to summarize it.
//!
//! A strategy is a function of the live `active` vec and the shared
//! [`Store`]. Its contract:
//!
//! 1. Persist every item it removes from `active` into the archive.
//! 2. Leave behind a representative summary plus any items that should stay
//!    hot (typically the most recent ones).
//!
//! Two strategies ship here:
//! - [`ThresholdCompaction`] — the generic, payload-agnostic compactor used
//!   by Tasks (any `Memory<T>`).
//! - [`OrchestratorCompaction`] — task-aware compaction for the Orchestrator's
//!   `Memory<OrchestratorEvent>`.

use std::collections::HashMap;
use std::time::SystemTime;

use anyhow::Result;

use super::TokenCount;
use super::memory::MemoryItem;
use super::store::{MemoryMeta, Store};

/// A compaction strategy operates on the live active vec in place.
///
/// It owns the decision of how many items to keep, how to summarize, and how
/// to populate archive metadata. Strategies MUST persist every removed item
/// before dropping it from `active`.
pub trait CompactionStrategy<T: MemoryItem>: Send + Sync {
    /// Compact `active` in place, archiving removed items into `archive`.
    /// `token_counter` is the same counter the owning [`super::Memory<T>`]
    /// uses, so strategies and the memory agree on token math.
    /// `max_active_tokens` is the owning Memory's active budget.
    fn compact(
        &self,
        active: &mut Vec<T>,
        archive: &Store,
        token_counter: &dyn TokenCount,
        max_active_tokens: usize,
    ) -> Result<()>;
}

/// Generic threshold-based compaction for any [`MemoryItem`].
///
/// When the active region exceeds `compact_at` percent of the budget, the
/// oldest items are archived and replaced by a single summary, leaving
/// `target` percent of the budget in the active region.
pub struct ThresholdCompaction {
    compact_at_percent: u32,
    target_percent: u32,
}

impl ThresholdCompaction {
    #[must_use]
    pub const fn new(compact_at_percent: u32, target_percent: u32) -> Self {
        Self {
            compact_at_percent,
            target_percent,
        }
    }

    fn pick_split(
        &self,
        active: &[impl MemoryItem],
        token_counter: &dyn TokenCount,
        max_active_tokens: usize,
    ) -> usize {
        let threshold = max_active_tokens * self.compact_at_percent as usize / 100;
        let active_tokens: usize = active
            .iter()
            .map(|i| token_counter.count_tokens(&i.to_text()))
            .sum();
        if active_tokens < threshold {
            return 0;
        }
        let target_budget = max_active_tokens * self.target_percent as usize / 100;

        let mut keep_from = active.len();
        let mut running = 0usize;
        for (idx, item) in active.iter().enumerate().rev() {
            let t = token_counter.count_tokens(&item.to_text());
            if running + t > target_budget && idx + 1 < active.len() {
                break;
            }
            running += t;
            keep_from = idx;
        }
        let mut split = keep_from.max(1);

        while split + 1 < active.len() {
            let kept_tokens: usize = active[split..]
                .iter()
                .map(|i| token_counter.count_tokens(&i.to_text()))
                .sum();
            let summary = format!("[{split} items compacted to archive]");
            let summary_tokens = token_counter.count_tokens(&summary);
            if kept_tokens + summary_tokens <= target_budget {
                break;
            }
            split += 1;
        }
        split
    }
}

impl<T: MemoryItem> CompactionStrategy<T> for ThresholdCompaction {
    fn compact(
        &self,
        active: &mut Vec<T>,
        archive: &Store,
        token_counter: &dyn TokenCount,
        max_active_tokens: usize,
    ) -> Result<()> {
        let split = self.pick_split(active.as_slice(), token_counter, max_active_tokens);
        if split == 0 {
            return Ok(());
        }

        let now = SystemTime::now();
        for item in &active[..] {
            let content = serde_json::to_string(item)?;
            let meta = MemoryMeta {
                timestamp: Some(now),
                searchable_text: item.to_text(),
                task_id: None,
                tags: Vec::new(),
            };
            archive.store(&content, &meta)?;
        }

        let summary = T::from_summary(&format!("[{split} items compacted to archive]"));

        let kept: Vec<T> = active[split..].to_vec();
        active.clear();
        active.push(summary);
        active.extend(kept);
        Ok(())
    }
}

/// Default threshold compactor (80% -> 50%).
#[must_use]
pub const fn default_threshold() -> ThresholdCompaction {
    ThresholdCompaction::new(80, 50)
}

/// Orchestrator-level event kind, used by [`OrchestratorCompaction`] to
/// classify items for task-aware compaction.
///
/// The full `TaskStatus` state machine lives in another module (task 4.1);
/// here we only need to distinguish "completed" highlights (compress to one
/// final summary) from in-progress highlights (rolling compression).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub enum EventKind {
    /// Periodic "still alive" status update from a Task.
    Heartbeat,
    /// A reported finding from a Task, tagged with whether the task is done.
    Highlight { task_completed: bool },
    /// Anything else (user messages, assistant turns, system events).
    Other,
}

/// An event stored in the Orchestrator's `Memory<OrchestratorEvent>`.
///
/// This is intentionally small: the Orchestrator-specific compaction strategy
/// only needs to categorize items by kind + task id and render them to text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub struct OrchestratorEvent {
    /// Discriminator used by [`OrchestratorCompaction`] to bucket items.
    pub kind: EventKindKind,
    /// Owning task id (all heartbeats/highlights carry one).
    pub task_id: String,
    /// Free-form event text. Summaries are computed from this.
    pub text: String,
}

/// Serialization-friendly mirror of [`EventKind`] (the original enum carries
/// no data the serde layer needs to hide, but exposing it directly would
/// couple serialization to enum variant names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub enum EventKindKind {
    Heartbeat,
    Highlight { task_completed: bool },
    Other,
}

impl OrchestratorEvent {
    #[must_use]
    pub const fn new(kind: EventKind, task_id: String, text: String) -> Self {
        let kind = match kind {
            EventKind::Heartbeat => EventKindKind::Heartbeat,
            EventKind::Highlight { task_completed } => EventKindKind::Highlight { task_completed },
            EventKind::Other => EventKindKind::Other,
        };
        Self {
            kind,
            task_id,
            text,
        }
    }

    #[must_use]
    pub const fn classifier(&self) -> EventKind {
        match self.kind {
            EventKindKind::Heartbeat => EventKind::Heartbeat,
            EventKindKind::Highlight { task_completed } => EventKind::Highlight { task_completed },
            EventKindKind::Other => EventKind::Other,
        }
    }
}

impl MemoryItem for OrchestratorEvent {
    fn to_text(&self) -> String {
        self.text.clone()
    }

    fn from_summary(summary: &str) -> Self {
        Self::new(
            EventKind::Other,
            "__summary__".to_string(),
            summary.to_string(),
        )
    }
}

/// Task-aware compaction for the Orchestrator's active memory.
///
/// Per the spec (`design.md` §D6):
/// - Completed task highlights → compress N highlights into 1 final summary.
/// - In-progress task old highlights → keep last 3, compress older into a
///   rolling summary.
/// - Heartbeats → keep only the latest per task, archive older ones.
///
/// Other items are left untouched in the active region. The strategy is
/// invoked on every Orchestrator turn; it is idempotent (a second run with
/// no new events is a no-op).
pub struct OrchestratorCompaction;

impl OrchestratorCompaction {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn summarize(items: &[&OrchestratorEvent]) -> String {
        let body = items
            .iter()
            .map(|e| format!("- [{}] {}", e.task_id, e.text))
            .collect::<Vec<_>>()
            .join("\n");
        format!("[compacted {} items]\n{body}", items.len())
    }
}

impl Default for OrchestratorCompaction {
    fn default() -> Self {
        Self::new()
    }
}

impl CompactionStrategy<OrchestratorEvent> for OrchestratorCompaction {
    fn compact(
        &self,
        active: &mut Vec<OrchestratorEvent>,
        archive: &Store,
        _token_counter: &dyn TokenCount,
        _max_active_tokens: usize,
    ) -> Result<()> {
        let now = SystemTime::now();

        let mut latest_heartbeat_per_task: HashMap<&str, usize> = HashMap::new();
        for (idx, ev) in active.iter().enumerate() {
            if ev.classifier() == EventKind::Heartbeat {
                latest_heartbeat_per_task
                    .entry(ev.task_id.as_str())
                    .and_modify(|best| *best = idx)
                    .or_insert(idx);
            }
        }
        let latest_set: std::collections::HashSet<usize> =
            latest_heartbeat_per_task.values().copied().collect();

        let mut highlight_buckets: HashMap<(&str, bool), Vec<usize>> = HashMap::new();
        for (idx, ev) in active.iter().enumerate() {
            if let EventKind::Highlight { task_completed } = ev.classifier() {
                highlight_buckets
                    .entry((ev.task_id.as_str(), task_completed))
                    .or_default()
                    .push(idx);
            }
        }

        let mut archive_idx: Vec<usize> = Vec::new();
        for ((_task, completed), indices) in &highlight_buckets {
            if *completed {
                archive_idx.extend(indices.iter().copied());
            } else {
                let keep_count = 3.min(indices.len());
                let (older, _recent) = indices.split_at(indices.len() - keep_count);
                archive_idx.extend(older.iter().copied());
            }
        }

        for (idx, ev) in active.iter().enumerate() {
            if ev.classifier() == EventKind::Heartbeat && !latest_set.contains(&idx) {
                archive_idx.push(idx);
            }
        }
        archive_idx.sort_unstable();
        archive_idx.dedup();

        for &idx in &archive_idx {
            let ev = &active[idx];
            let content = serde_json::to_string(ev)?;
            let meta = MemoryMeta {
                timestamp: Some(now),
                task_id: Some(ev.task_id.clone()),
                searchable_text: ev.text.clone(),
                tags: vec![kind_tag(ev.classifier())],
            };
            archive.store(&content, &meta)?;
        }

        let mut completed_summaries: Vec<OrchestratorEvent> = Vec::new();
        for ((task, completed), indices) in &highlight_buckets {
            if *completed && !indices.is_empty() {
                let summary = OrchestratorEvent::new(
                    EventKind::Highlight {
                        task_completed: true,
                    },
                    (*task).to_string(),
                    Self::summarize(&indices.iter().map(|&i| &active[i]).collect::<Vec<_>>()),
                );
                completed_summaries.push(summary);
            }
        }

        // Survivors first (in original order), then completed-task summaries.
        // Exact summary position is not load-bearing — the Orchestrator
        // renders the whole active region as context regardless of order.
        let mut new_active: Vec<OrchestratorEvent> = Vec::with_capacity(active.len());
        for (idx, ev) in active.iter().enumerate() {
            if archive_idx.contains(&idx) {
                continue;
            }
            new_active.push(ev.clone());
        }
        new_active.extend(completed_summaries);

        *active = new_active;
        Ok(())
    }
}

fn kind_tag(kind: EventKind) -> String {
    match kind {
        EventKind::Heartbeat => "heartbeat".to_string(),
        EventKind::Highlight { task_completed } => {
            if task_completed {
                "highlight-completed".to_string()
            } else {
                "highlight-in-progress".to_string()
            }
        }
        EventKind::Other => "other".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: EventKind, task: &str, text: &str) -> OrchestratorEvent {
        OrchestratorEvent::new(kind, task.to_string(), text.to_string())
    }

    #[test]
    fn orchestrator_keeps_latest_heartbeat_per_task() {
        let store = Store::in_memory().unwrap();
        let mut active = vec![
            ev(EventKind::Heartbeat, "A", "h1"),
            ev(EventKind::Heartbeat, "A", "h2"),
            ev(EventKind::Heartbeat, "A", "h3"),
            ev(EventKind::Heartbeat, "B", "b1"),
        ];
        OrchestratorCompaction::new()
            .compact(
                &mut active,
                &store,
                &crate::memory::WordCountTokenizer,
                10_000,
            )
            .unwrap();

        let heartbeats: Vec<_> = active
            .iter()
            .filter(|e| e.classifier() == EventKind::Heartbeat)
            .collect();
        assert_eq!(heartbeats.len(), 2);
        assert!(heartbeats.iter().any(|e| e.text == "h3"));
        assert!(heartbeats.iter().any(|e| e.text == "b1"));

        assert!(store.count().unwrap() >= 2);
    }

    #[test]
    fn orchestrator_compresses_completed_task_highlights() {
        let store = Store::in_memory().unwrap();
        let mut active: Vec<OrchestratorEvent> = (0..5)
            .map(|i| {
                ev(
                    EventKind::Highlight {
                        task_completed: true,
                    },
                    "A",
                    &format!("highlight {i}"),
                )
            })
            .collect();

        OrchestratorCompaction::new()
            .compact(
                &mut active,
                &store,
                &crate::memory::WordCountTokenizer,
                10_000,
            )
            .unwrap();

        let highlights = active
            .iter()
            .filter(|e| matches!(e.classifier(), EventKind::Highlight { .. }))
            .count();
        assert_eq!(highlights, 1);
        assert_eq!(store.count().unwrap(), 5);
    }

    #[test]
    fn orchestrator_rolling_compresses_in_progress_highlights() {
        let store = Store::in_memory().unwrap();
        let mut active: Vec<OrchestratorEvent> = (0..6)
            .map(|i| {
                ev(
                    EventKind::Highlight {
                        task_completed: false,
                    },
                    "A",
                    &format!("highlight {i}"),
                )
            })
            .collect();

        OrchestratorCompaction::new()
            .compact(
                &mut active,
                &store,
                &crate::memory::WordCountTokenizer,
                10_000,
            )
            .unwrap();

        let kept: Vec<_> = active
            .iter()
            .filter(|e| matches!(e.classifier(), EventKind::Highlight { .. }))
            .collect();
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].text, "highlight 3");
        assert_eq!(kept[2].text, "highlight 5");
        assert_eq!(store.count().unwrap(), 3);
    }

    #[test]
    fn orchestrator_is_idempotent() {
        let store = Store::in_memory().unwrap();
        let mut active = vec![
            ev(EventKind::Heartbeat, "A", "h1"),
            ev(EventKind::Other, "A", "msg"),
        ];
        OrchestratorCompaction::new()
            .compact(
                &mut active,
                &store,
                &crate::memory::WordCountTokenizer,
                10_000,
            )
            .unwrap();
        let snapshot = active.clone();
        OrchestratorCompaction::new()
            .compact(
                &mut active,
                &store,
                &crate::memory::WordCountTokenizer,
                10_000,
            )
            .unwrap();
        assert_eq!(active, snapshot);
    }
}
