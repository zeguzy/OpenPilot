//! Memory system — generic `Memory<T>` with active + archive regions.
//!
//! See `design.md` §D6 for the fractal design rationale and
//! `specs/memory-system/spec.md` for the requirement contracts.
//!
//! # Quick tour
//!
//! ```
//! use opca_core::memory::{Memory, RecallQuery};
//!
//! let mut mem = Memory::<String>::new_in_memory(1000).unwrap();
//! mem.push("the auth module needs refactor".to_string());
//! mem.remember(&"auth refactor summary".to_string(), &["auth", "security"]).unwrap();
//! let hits = mem.recall(&RecallQuery::Keyword("auth".into())).unwrap();
//! assert_eq!(hits.len(), 1);
//! ```

mod compact;
mod index;
#[allow(clippy::module_inception)]
mod memory;
mod store;

pub use compact::{
    CompactionStrategy, EventKind, EventKindKind, OrchestratorCompaction, OrchestratorEvent,
    ThresholdCompaction, default_threshold,
};
pub use index::{RecallQuery, extract_keywords};
pub use memory::{Memory, MemoryItem, TokenCount, WordCountTokenizer};
pub use store::{ArchivedRecord, MemoryMeta, Store};
