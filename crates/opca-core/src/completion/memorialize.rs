//! Memorialize stage (Task 12.7).
//!
//! After Merge succeeds, the Memorialize stage:
//! 1. Stores the final summary in the Orchestrator's memory (via `remember`).
//! 2. Archives the full context + diff in the Cold Store with indices
//!    (keyword, time, `task_id`, tags, files modified).
//! 3. Merges highlights into the Orchestrator context.
//! 4. Triggers Orchestrator compaction if needed.
//!
//! See `design.md` §D9 (④ Memorialize) and `specs/completion-pipeline/spec.md`.

use anyhow::Result;

use crate::focus::Highlight;
use crate::memory::{EventKind, MemoryMeta, OrchestratorEvent, RecallQuery, Store};
use crate::orchestrator::Orchestrator;
use crate::provider::Message;
use crate::workspace::ChangeSet;

/// Input bundle for Memorialize: everything that should be archived.
#[derive(Debug, Clone)]
pub struct MemorializeInput<'a> {
    pub task_id: &'a str,
    pub final_summary: &'a str,
    pub active: &'a [Message],
    pub diff: &'a ChangeSet,
    pub highlights: &'a [Highlight],
    pub tags: &'a [&'a str],
}

/// Store the Task's final summary in the Orchestrator's memory and archive
/// the full context + diff into the Cold Store.
///
/// After this call, items are recallable via [`RecallQuery::TaskId`].
pub fn memorialize(orchestrator: &Orchestrator, input: &MemorializeInput<'_>) -> Result<()> {
    let summary_event = OrchestratorEvent::new(
        EventKind::Highlight {
            task_completed: true,
        },
        input.task_id.to_string(),
        format!("[final summary] {}", input.final_summary),
    );
    let mut summary_tags: Vec<&str> = input.tags.to_vec();
    summary_tags.push("final-summary");
    orchestrator.remember(&summary_event, &summary_tags)?;

    let context_text = build_context_text(input.active, input.diff);
    let context_event =
        OrchestratorEvent::new(EventKind::Other, input.task_id.to_string(), context_text);
    let file_tags: Vec<String> = files_of(input.diff);
    let file_tag_refs: Vec<&str> = file_tags.iter().map(String::as_str).collect();
    let mut ctx_tags: Vec<&str> = vec!["full-context", "memorialized"];
    ctx_tags.extend_from_slice(&file_tag_refs);
    ctx_tags.extend_from_slice(input.tags);
    orchestrator.remember(&context_event, &ctx_tags)?;

    for hl in input.highlights {
        let ev = OrchestratorEvent::new(
            EventKind::Highlight {
                task_completed: true,
            },
            input.task_id.to_string(),
            format!("[{}] {}", hl.tag, hl.summary),
        );
        orchestrator.remember(&ev, &[hl.tag.as_str()])?;
    }

    Ok(())
}

fn build_context_text(active: &[Message], diff: &ChangeSet) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for m in active {
        if !m.text().is_empty() {
            let role = match m.role {
                crate::provider::MessageRole::User => "user",
                crate::provider::MessageRole::Assistant => "assistant",
                crate::provider::MessageRole::System => "system",
                crate::provider::MessageRole::Tool => "tool",
            };
            let _ = writeln!(text, "{role}: {}", m.text());
        }
    }
    let files = files_of(diff);
    if !files.is_empty() {
        let _ = writeln!(text, "Files modified: {}", files.join(", "));
    }
    text
}

fn files_of(diff: &ChangeSet) -> Vec<String> {
    diff.added
        .iter()
        .chain(diff.modified.iter())
        .chain(diff.deleted.iter())
        .map(|p| format!("file:{}", p.display()))
        .collect()
}

/// Standalone helper: archive a single text item with `task_id` index, no
/// Orchestrator required. Used by tests and by the simpler "summary-only"
/// memorialization path.
pub fn archive_summary(store: &Store, task_id: &str, summary: &str, tags: &[&str]) -> Result<i64> {
    let meta = MemoryMeta {
        timestamp: None,
        task_id: Some(task_id.to_string()),
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        searchable_text: summary.to_string(),
    };
    store.store(summary, &meta)
}

/// Recall all archived items for a given `task_id`. Used by tests to verify
/// Memorialize stored items correctly.
pub fn recall_by_task_id(store: &Store, task_id: &str) -> Result<Vec<String>> {
    let records = store.recall(&RecallQuery::TaskId(task_id.to_string()))?;
    Ok(records.into_iter().map(|r| r.content).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_and_recall_summary_by_task_id() {
        let store = Store::in_memory().unwrap();
        archive_summary(&store, "task-A", "auth refactor done", &["security"]).unwrap();

        let hits = recall_by_task_id(&store, "task-A").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], "auth refactor done");
    }

    #[test]
    fn archive_separates_tasks() {
        let store = Store::in_memory().unwrap();
        archive_summary(&store, "task-A", "summary A", &[]).unwrap();
        archive_summary(&store, "task-B", "summary B", &[]).unwrap();

        let a = recall_by_task_id(&store, "task-A").unwrap();
        let b = recall_by_task_id(&store, "task-B").unwrap();
        let all = store.all().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn archive_recall_by_keyword() {
        let store = Store::in_memory().unwrap();
        archive_summary(&store, "task-X", "refactored OAuth2 flow", &[]).unwrap();

        let records = store
            .recall(&RecallQuery::Keyword("oauth2".into()))
            .unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn archive_recall_by_tag() {
        let store = Store::in_memory().unwrap();
        archive_summary(&store, "task-Y", "security audit", &["security"]).unwrap();

        let records = store.recall(&RecallQuery::Tag("security".into())).unwrap();
        assert_eq!(records.len(), 1);
    }
}
