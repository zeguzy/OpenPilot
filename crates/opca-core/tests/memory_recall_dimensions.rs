//! Task 3.9 integration tests: recall across all dimensions simultaneously.
//!
//! Store items with various tags / `task_ids` / timestamps, then query each
//! dimension independently and verify the expected subset is returned.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opca_core::memory::{Memory, MemoryMeta, RecallQuery};

fn t(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn remember_with_meta(
    mem: &Memory<String>,
    text: &str,
    timestamp: SystemTime,
    task_id: Option<&str>,
    tags: &[&str],
) {
    mem.remember_with(
        &text.to_string(),
        &MemoryMeta {
            timestamp: Some(timestamp),
            task_id: task_id.map(String::from),
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            searchable_text: text.to_string(),
        },
    )
    .unwrap();
}

#[test]
fn recall_by_keyword_dimension() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(&mem, "auth refactor summary", t(1000), None, &[]);
    remember_with_meta(&mem, "network timeout debug", t(2000), None, &[]);
    remember_with_meta(&mem, "auth token rotation", t(3000), None, &[]);

    let hits = mem.recall(&RecallQuery::Keyword("auth".into())).unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn recall_by_time_range_dimension() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(&mem, "early event", t(1000), None, &[]);
    remember_with_meta(&mem, "middle event", t(2000), None, &[]);
    remember_with_meta(&mem, "late event", t(3000), None, &[]);

    let mid = mem
        .recall(&RecallQuery::TimeRange {
            from: t(1500),
            to: t(2500),
        })
        .unwrap();
    assert_eq!(mid.len(), 1);
    assert_eq!(mid[0], "middle event");

    let all = mem
        .recall(&RecallQuery::TimeRange {
            from: t(0),
            to: t(10_000),
        })
        .unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn recall_by_task_id_dimension() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(&mem, "task A step 1", t(1000), Some("task-A"), &[]);
    remember_with_meta(&mem, "task B step 1", t(2000), Some("task-B"), &[]);
    remember_with_meta(&mem, "task A step 2", t(3000), Some("task-A"), &[]);

    let a = mem.recall(&RecallQuery::TaskId("task-A".into())).unwrap();
    assert_eq!(a.len(), 2);
    let b = mem.recall(&RecallQuery::TaskId("task-B".into())).unwrap();
    assert_eq!(b.len(), 1);
    let c = mem.recall(&RecallQuery::TaskId("task-C".into())).unwrap();
    assert!(c.is_empty());
}

#[test]
fn recall_by_tag_dimension() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(
        &mem,
        "security audit finding",
        t(1000),
        None,
        &["security", "audit"],
    );
    remember_with_meta(&mem, "perf benchmark result", t(2000), None, &["perf"]);
    remember_with_meta(&mem, "security risk noted", t(3000), None, &["security"]);

    let sec = mem.recall(&RecallQuery::Tag("security".into())).unwrap();
    assert_eq!(sec.len(), 2);
    let aud = mem.recall(&RecallQuery::Tag("audit".into())).unwrap();
    assert_eq!(aud.len(), 1);
}

#[test]
fn recall_semantic_falls_back_to_keyword() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(&mem, "database migration plan", t(1000), None, &[]);

    let hits = mem
        .recall(&RecallQuery::Semantic("database".into()))
        .unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn multi_dimensional_cross_filter_setup() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();

    remember_with_meta(
        &mem,
        "task-A security finding at dawn",
        t(1000),
        Some("task-A"),
        &["security"],
    );
    remember_with_meta(
        &mem,
        "task-B perf note at noon",
        t(2000),
        Some("task-B"),
        &["perf"],
    );
    remember_with_meta(
        &mem,
        "task-A security followup at dusk",
        t(3000),
        Some("task-A"),
        &["security"],
    );
    remember_with_meta(
        &mem,
        "task-B security cross-note",
        t(4000),
        Some("task-B"),
        &["security"],
    );

    let by_task_a = mem.recall(&RecallQuery::TaskId("task-A".into())).unwrap();
    assert_eq!(by_task_a.len(), 2);

    let by_task_b = mem.recall(&RecallQuery::TaskId("task-B".into())).unwrap();
    assert_eq!(by_task_b.len(), 2);

    let by_tag_sec = mem.recall(&RecallQuery::Tag("security".into())).unwrap();
    assert_eq!(by_tag_sec.len(), 3);

    let by_tag_perf = mem.recall(&RecallQuery::Tag("perf".into())).unwrap();
    assert_eq!(by_tag_perf.len(), 1);

    let by_kw = mem
        .recall(&RecallQuery::Keyword("security".into()))
        .unwrap();
    assert_eq!(by_kw.len(), 3);

    let by_time = mem
        .recall(&RecallQuery::TimeRange {
            from: t(0),
            to: t(1500),
        })
        .unwrap();
    assert_eq!(by_time.len(), 1);
}

#[test]
fn empty_query_returns_all_items() {
    let mem = Memory::<String>::new_in_memory(1000).unwrap();
    remember_with_meta(&mem, "first", t(1000), None, &[]);
    remember_with_meta(&mem, "second", t(2000), None, &[]);

    let all = mem.recall(&RecallQuery::Keyword(String::new())).unwrap();
    assert_eq!(all.len(), 2);
}
