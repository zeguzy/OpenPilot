//! Cold Store persistence (Task 15.5).
//!
//! The Cold Store is the cross-session long-term memory: a [`Store`] backed
//! by `<agent_dir>/cold-store.sqlite`. Items archived in a previous session
//! (summaries, highlights, finalized Task artifacts) survive session end and
//! remain recallable from later sessions.
//!
//! See `design.md` §Cold Store: the same `Memory<SessionSummary>` fractal
//! shape as active/archive, just with a longer-lived file path. This module
//! provides the path convention + a one-line loader; the heavy lifting
//! (schema, recall dimensions) is inherited from [`crate::memory::Store`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::memory::Store;

/// The Cold Store file name under the agent data dir.
pub const COLD_STORE_FILE: &str = "cold-store.sqlite";

/// Resolve `<agent_dir>/cold-store.sqlite`.
///
/// `agent_dir` is typically `<project>/.agent`. The path is returned even if
/// the file does not yet exist — [`load_cold_store`] will create it.
#[must_use]
pub fn cold_store_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join(COLD_STORE_FILE)
}

/// Open (or create) the Cold Store at `<agent_dir>/cold-store.sqlite`.
///
/// The agent dir and any missing parent directories are created as needed.
/// Returns a [`Store`] that can be queried with any [`crate::memory::RecallQuery`]
/// — keyword, tag, time-range, or task-id — to surface items archived in
/// earlier sessions.
pub fn load_cold_store(agent_dir: &Path) -> Result<Store> {
    std::fs::create_dir_all(agent_dir)
        .with_context(|| format!("creating agent dir {}", agent_dir.display()))?;
    let path = cold_store_path(agent_dir);
    Store::open(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryMeta, RecallQuery};

    #[test]
    fn cold_store_path_is_agent_dir_child() {
        let p = cold_store_path(Path::new("/repo/.agent"));
        assert_eq!(p.file_name().unwrap(), COLD_STORE_FILE);
    }

    #[test]
    fn load_cold_store_persists_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let agent_dir = tmp.path().join(".agent");

        // First "session": write a tagged item.
        {
            let store = load_cold_store(&agent_dir).unwrap();
            store
                .store(
                    "auth summary from previous session",
                    &MemoryMeta {
                        searchable_text: "auth refactor summary".into(),
                        tags: vec!["auth".into(), "security".into()],
                        ..Default::default()
                    },
                )
                .unwrap();
        }

        // Cold store file exists.
        assert!(cold_store_path(&agent_dir).exists());

        // Second "session": reopen and recall the previously-stored item.
        let store = load_cold_store(&agent_dir).unwrap();
        let hits = store.recall(&RecallQuery::Keyword("auth".into())).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "auth summary from previous session");
    }

    #[test]
    fn load_cold_store_creates_missing_agent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let agent_dir = tmp.path().join("nested").join(".agent");
        assert!(!agent_dir.exists());
        let store = load_cold_store(&agent_dir).unwrap();
        // Smoke-store to confirm it's usable.
        store.store("x", &MemoryMeta::from_text("x")).unwrap();
        assert!(agent_dir.exists());
    }
}
