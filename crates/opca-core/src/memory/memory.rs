//! The generic [`Memory<T>`] component — a fractal container with an
//! active region (current context window) and an archive region
//! (persistent [`Store`]).
//!
//! The same `Memory<T>` is reused at every level of the agent stack:
//! - `Memory<Message>` inside a Task (active = loop context)
//! - `Memory<ConversationEvent>` in the Orchestrator
//! - `Memory<SessionSummary>` in the cross-session Cold Store
//!
//! See `design.md` §D6 for the fractal design rationale.

use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;

use super::compact::CompactionStrategy;
use super::index::RecallQuery;
use super::store::{MemoryMeta, Store};

/// Approximate token counter interface.
///
/// A real BPE tokenizer will be wired in later; for now [`WordCountTokenizer`]
/// provides a deterministic word-count proxy that is good enough for unit
/// testing compaction thresholds.
pub trait TokenCount: Send + Sync {
    /// Return the approximate token count of `text`.
    fn count_tokens(&self, text: &str) -> usize;
}

/// Word-count proxy tokenizer. Counts runs of whitespace-separated tokens.
#[derive(Debug, Default, Clone, Copy)]
#[allow(clippy::module_name_repetitions)]
pub struct WordCountTokenizer;

impl TokenCount for WordCountTokenizer {
    fn count_tokens(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

/// Behaviors a `Memory<T>` requires of its payload type `T`.
///
/// `T` must be serializable (for archive storage), but it also needs to render
/// to searchable text and be reconstructable from a summary string during
/// compaction. We bundle those three concerns into one trait so callers don't
/// have to thread three separate bounds through every generic.
///
/// This is implemented for the common test case `String`.
pub trait MemoryItem: Serialize + DeserializeOwned + Clone + Send + Sync {
    /// Render the item as searchable / countable text.
    fn to_text(&self) -> String;

    /// Build an item that carries a compaction summary. The returned item is
    /// placed back into the active region; the original items are archived.
    fn from_summary(summary: &str) -> Self;
}

impl MemoryItem for String {
    fn to_text(&self) -> String {
        self.clone()
    }

    fn from_summary(summary: &str) -> Self {
        summary.to_string()
    }
}

/// The fractal memory component.
///
/// Holds an `active` region (the live context window) and an `archive`
/// [`Store`] (persistent, indexed). Items move from active to archive via
/// [`Memory::compact`].
///
/// # Type parameters
/// - `T` — the payload type. Must implement [`MemoryItem`].
pub struct Memory<T: MemoryItem> {
    active: Vec<T>,
    archive: Store,
    token_counter: Box<dyn TokenCount>,
    max_active_tokens: usize,
}

impl<T: MemoryItem> Memory<T> {
    /// Build a `Memory` over an in-memory `SQLite` archive. Intended for tests.
    pub fn new_in_memory(max_active_tokens: usize) -> Result<Self> {
        Ok(Self::with_store(Store::in_memory()?, max_active_tokens))
    }

    /// Build a `Memory` over a file-backed `SQLite` archive. Intended for prod.
    pub fn open(path: &Path, max_active_tokens: usize) -> Result<Self> {
        Ok(Self::with_store(Store::open(path)?, max_active_tokens))
    }

    /// Build a `Memory` over a pre-constructed [`Store`].
    #[must_use]
    pub fn with_store(archive: Store, max_active_tokens: usize) -> Self {
        Self {
            active: Vec::new(),
            archive,
            token_counter: Box::new(WordCountTokenizer),
            max_active_tokens,
        }
    }

    /// Override the default [`WordCountTokenizer`] with a real tokenizer.
    pub fn with_token_counter(mut self, counter: Box<dyn TokenCount>) -> Self {
        self.token_counter = counter;
        self
    }

    /// Append an item to the active region without touching the archive.
    pub fn push(&mut self, item: T) {
        self.active.push(item);
    }

    /// Borrow the live active region.
    #[must_use]
    pub fn active_slice(&self) -> &[T] {
        &self.active
    }

    /// Mutable access to the live active region (used by compaction
    /// strategies and tests).
    #[allow(clippy::missing_const_for_fn)]
    pub fn active_mut(&mut self) -> &mut Vec<T> {
        &mut self.active
    }

    /// Borrow the underlying archive store.
    #[must_use]
    pub const fn archive(&self) -> &Store {
        &self.archive
    }

    /// Maximum active token budget.
    #[must_use]
    pub const fn max_active_tokens(&self) -> usize {
        self.max_active_tokens
    }

    /// Current token count of the active region, summed across all items.
    pub fn active_tokens(&self) -> usize {
        self.active
            .iter()
            .map(|i| self.token_counter.count_tokens(&i.to_text()))
            .sum()
    }

    /// `true` when the active region has reached the compaction threshold
    /// (80% of `max_active_tokens`). This is the trigger the agent loop polls
    /// before invoking [`Memory::compact`].
    #[must_use]
    pub fn is_near_limit(&self) -> bool {
        self.active_tokens() * 5 >= self.max_active_tokens * 4
    }

    /// Store an item directly into the archive, building indices from
    /// `tags` plus auto-extracted keywords.
    ///
    /// The item is *not* added to the active region.
    pub fn remember(&self, item: &T, tags: &[&str]) -> Result<i64> {
        let content = serde_json::to_string(item)?;
        let meta = MemoryMeta {
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            searchable_text: item.to_text(),
            timestamp: Some(SystemTime::now()),
            task_id: None,
        };
        self.archive.store(&content, &meta)
    }

    /// Store an item into the archive with full metadata (timestamp, `task_id`,
    /// tags). Used by compaction strategies and Orchestrator bookkeeping.
    pub fn remember_with(&self, item: &T, meta: &MemoryMeta) -> Result<i64> {
        let content = serde_json::to_string(item)?;
        self.archive.store(&content, meta)
    }

    /// Query the archive along one [`RecallQuery`] dimension, deserializing
    /// each matching record back into `T`.
    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<T>> {
        let records = self.archive.recall(query)?;
        records
            .into_iter()
            .map(|r| serde_json::from_str::<T>(&r.content).map_err(anyhow::Error::from))
            .collect()
    }

    /// Number of items currently archived.
    pub fn archive_len(&self) -> Result<i64> {
        self.archive.count()
    }

    /// Run a compaction strategy against the active region.
    ///
    /// Typical strategies move older items into the archive and replace them
    /// with a summary. The contract is that every item removed from `active`
    /// must already be persisted in the archive — strategies enforce this.
    pub fn compact(&mut self, strategy: &dyn CompactionStrategy<T>) -> Result<()> {
        strategy.compact(
            &mut self.active,
            &self.archive,
            &*self.token_counter,
            self.max_active_tokens,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::compact::ThresholdCompaction;
    use super::*;

    #[test]
    fn push_and_active_slice() {
        let mut mem = Memory::<String>::new_in_memory(1000).unwrap();
        mem.push("alpha".to_string());
        mem.push("beta".to_string());
        assert_eq!(mem.active_slice().len(), 2);
        assert_eq!(mem.active_slice()[0], "alpha");
    }

    #[test]
    fn token_count_sums_active_items() {
        let mut mem = Memory::<String>::new_in_memory(1000).unwrap();
        mem.push("one two three".to_string()); // 3 tokens
        mem.push("four five".to_string()); // 2 tokens
        assert_eq!(mem.active_tokens(), 5);
    }

    #[test]
    fn is_near_limit_at_80_percent() {
        let mut mem = Memory::<String>::new_in_memory(100).unwrap();
        mem.push("x ".repeat(80));
        assert!(mem.is_near_limit());

        let mut mem2 = Memory::<String>::new_in_memory(100).unwrap();
        mem2.push("x ".repeat(79));
        assert!(!mem2.is_near_limit());
    }

    #[test]
    fn compact_threshold_moves_items_to_archive() {
        let mut mem = Memory::<String>::new_in_memory(100).unwrap();
        for i in 0..50 {
            mem.push(format!("item number {i} with some filler words"));
        }
        assert!(mem.is_near_limit());

        mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

        assert!(
            mem.archive_len().unwrap() > 0,
            "some items should be archived"
        );
        assert!(
            mem.active_tokens() < 400,
            "active should be reduced significantly"
        );
        assert!(
            !mem.active_slice().is_empty(),
            "active should retain recent items + summary"
        );
    }
}
