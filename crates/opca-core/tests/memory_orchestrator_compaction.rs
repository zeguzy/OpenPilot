//! Task 3.10: Orchestrator-specific compaction strategy.
//!
//! Scenarios from the memory-system spec:
//! - Completed task highlights → N highlights compressed into 1 final summary
//!   (all originals preserved in archive).
//! - In-progress task highlights → keep last 3, compress older (rolling).
//! - Heartbeats → keep only the latest per task, archive older.

use opca_core::memory::{
    EventKind, Memory, OrchestratorCompaction, OrchestratorEvent, RecallQuery, ThresholdCompaction,
};

fn hb(task: &str, text: &str) -> OrchestratorEvent {
    OrchestratorEvent::new(EventKind::Heartbeat, task.to_string(), text.to_string())
}

fn hl_done(task: &str, text: &str) -> OrchestratorEvent {
    OrchestratorEvent::new(
        EventKind::Highlight {
            task_completed: true,
        },
        task.to_string(),
        text.to_string(),
    )
}

fn hl_progress(task: &str, text: &str) -> OrchestratorEvent {
    OrchestratorEvent::new(
        EventKind::Highlight {
            task_completed: false,
        },
        task.to_string(),
        text.to_string(),
    )
}

fn other(task: &str, text: &str) -> OrchestratorEvent {
    OrchestratorEvent::new(EventKind::Other, task.to_string(), text.to_string())
}

#[test]
fn completed_task_highlights_compress_to_one_summary() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    for i in 0..5 {
        mem.push(hl_done("task-A", &format!("highlight {i}")));
    }

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    let highlights = mem
        .active_slice()
        .iter()
        .filter(|e| matches!(e.classifier(), EventKind::Highlight { .. }))
        .count();
    assert_eq!(highlights, 1);

    let archived = mem
        .recall(&RecallQuery::Tag("highlight-completed".into()))
        .unwrap();
    assert_eq!(archived.len(), 5);
}

#[test]
fn in_progress_highlights_rolling_compression_keeps_last_three() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    for i in 0..6 {
        mem.push(hl_progress("task-A", &format!("highlight {i}")));
    }

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    let kept: Vec<_> = mem
        .active_slice()
        .iter()
        .filter(|e| matches!(e.classifier(), EventKind::Highlight { .. }))
        .collect();
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0].text, "highlight 3");
    assert_eq!(kept[2].text, "highlight 5");

    let archived = mem
        .recall(&RecallQuery::Tag("highlight-in-progress".into()))
        .unwrap();
    assert_eq!(archived.len(), 3);
}

#[test]
fn heartbeats_keep_only_latest_per_task() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    for i in 0..10 {
        mem.push(hb("task-A", &format!("heartbeat {i}")));
    }
    mem.push(hb("task-B", "b1"));

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    let hbs: Vec<_> = mem
        .active_slice()
        .iter()
        .filter(|e| e.classifier() == EventKind::Heartbeat)
        .collect();
    assert_eq!(hbs.len(), 2);
    assert!(hbs.iter().any(|e| e.text == "heartbeat 9"));
    assert!(hbs.iter().any(|e| e.text == "b1"));

    let archived = mem.recall(&RecallQuery::Tag("heartbeat".into())).unwrap();
    assert_eq!(archived.len(), 9);
}

#[test]
fn orchestrator_compaction_leaves_other_events_untouched() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    mem.push(other("user", "what's running?"));
    mem.push(hb("task-A", "alive"));
    mem.push(other("orch", "dispatching task-B"));

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    let others = mem
        .active_slice()
        .iter()
        .filter(|e| e.classifier() == EventKind::Other)
        .count();
    assert_eq!(others, 2);
}

#[test]
fn orchestrator_compaction_idempotent() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    mem.push(hb("task-A", "h1"));
    mem.push(hl_progress("task-A", "finding 1"));

    mem.compact(&OrchestratorCompaction::new()).unwrap();
    let snapshot = mem.active_slice().to_vec();

    mem.compact(&OrchestratorCompaction::new()).unwrap();
    assert_eq!(mem.active_slice(), snapshot);
}

#[test]
fn orchestrator_compaction_preserves_data_in_archive() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    let labels = ["alpha", "beta", "gamma", "delta", "epsilon"];
    for label in &labels {
        mem.push(hl_done("task-A", &format!("unique-finding-{label}")));
    }

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    for label in &labels {
        let hits = mem
            .recall(&RecallQuery::Keyword((*label).to_string()))
            .unwrap();
        assert_eq!(hits.len(), 1, "finding {label} lost");
    }
}

#[test]
fn orchestrator_compaction_works_with_mixed_events() {
    let mut mem = Memory::<OrchestratorEvent>::new_in_memory(10_000).unwrap();
    mem.push(hb("A", "a-hb-1"));
    mem.push(hb("A", "a-hb-2"));
    mem.push(hl_progress("A", "a-progress-1"));
    mem.push(hl_progress("A", "a-progress-2"));
    mem.push(hl_done("B", "b-done-1"));
    mem.push(hl_done("B", "b-done-2"));
    mem.push(other("user", "question"));

    mem.compact(&OrchestratorCompaction::new()).unwrap();

    let active = mem.active_slice();
    let hb_count = active
        .iter()
        .filter(|e| e.classifier() == EventKind::Heartbeat)
        .count();
    assert_eq!(hb_count, 1);

    let done_hl = active
        .iter()
        .filter(|e| {
            matches!(
                e.classifier(),
                EventKind::Highlight {
                    task_completed: true
                }
            )
        })
        .count();
    assert_eq!(done_hl, 1);

    let prog_hl = active
        .iter()
        .filter(|e| {
            matches!(
                e.classifier(),
                EventKind::Highlight {
                    task_completed: false
                }
            )
        })
        .count();
    assert_eq!(prog_hl, 2);
}

#[test]
fn threshold_compaction_also_works_for_orchestrator_when_generic() {
    let mut mem = Memory::<String>::new_in_memory(100).unwrap();
    for i in 0..50 {
        mem.push(format!("orchestrator event {i} padding text"));
    }

    mem.compact(&ThresholdCompaction::new(80, 50)).unwrap();

    assert_eq!(mem.archive_len().unwrap(), 50);
}
