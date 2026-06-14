//! Task 17.6 — E2E: Orchestrator recall retrieves info from previous session.
//!
//! Simulates two sessions sharing the same Cold Store file:
//! - Session 1: writes a Task summary + highlight into the Cold Store.
//! - Session 2: reopens the Cold Store and recalls the prior session's data
//!   by keyword, tag, and `task_id`.

use opca_core::memory::{MemoryMeta, RecallQuery};
use opca_core::session::cold_store::load_cold_store;

#[test]
#[ignore = "E2E: cold store cross-session recall"]
fn e2e_cold_store_cross_session_recall() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let agent_dir = tmp.path().join(".agent");

    {
        let store = load_cold_store(&agent_dir).expect("open cold store session 1");

        store
            .store(
                "Refactored OAuth2 authentication flow",
                &MemoryMeta {
                    searchable_text: "auth oauth2 refactor security".into(),
                    tags: vec!["auth".into(), "security".into()],
                    task_id: Some("task-session-1".into()),
                    ..Default::default()
                },
            )
            .expect("store session 1 item");

        store
            .store(
                "Fixed memory leak in connection pool",
                &MemoryMeta {
                    searchable_text: "memory leak connection pool performance".into(),
                    tags: vec!["performance".into()],
                    task_id: Some("task-session-1-leak".into()),
                    ..Default::default()
                },
            )
            .expect("store session 1 leak item");
    }

    assert!(
        agent_dir.join("cold-store.sqlite").exists(),
        "cold store file should persist on disk"
    );

    let store = load_cold_store(&agent_dir).expect("open cold store session 2");

    let auth_hits = store
        .recall(&RecallQuery::Keyword("oauth2".into()))
        .expect("recall by keyword");
    assert_eq!(auth_hits.len(), 1);
    assert!(
        auth_hits[0]
            .content
            .contains("Refactored OAuth2 authentication flow")
    );

    let security_hits = store
        .recall(&RecallQuery::Tag("security".into()))
        .expect("recall by tag");
    assert_eq!(security_hits.len(), 1);

    let perf_hits = store
        .recall(&RecallQuery::Tag("performance".into()))
        .expect("recall by tag performance");
    assert_eq!(perf_hits.len(), 1);
    assert!(perf_hits[0].content.contains("memory leak"));

    let task_hits = store
        .recall(&RecallQuery::TaskId("task-session-1".into()))
        .expect("recall by task id");
    assert_eq!(task_hits.len(), 1);
    assert!(task_hits[0].content.contains("OAuth2"));

    let all = store.all().expect("store all");
    assert_eq!(all.len(), 2, "cold store should contain both items");
}
