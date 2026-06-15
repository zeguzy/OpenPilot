use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use opca_core::di::{Clock, StdFileSystem, StdProcess};
use opca_core::focus::{FocusUpdate, Highlight, Severity};
use opca_core::lifecycle::TaskStatus;
use opca_core::memory::{EventKind, OrchestratorEvent, RecallQuery};
use opca_core::orchestrator::{Orchestrator, RouteDecision, predict_conflict, route};
use opca_core::provider::{Message, Provider};
use opca_test_utils::ScriptedProvider;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_orchestrator(
    provider: Arc<dyn Provider>,
    clock: Arc<dyn Clock>,
) -> (Orchestrator, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("placeholder.txt"), b"x").expect("write");
    let orch = Orchestrator::new(
        provider,
        tmp.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    (orch, tmp)
}

fn make_heartbeat(
    task_id: &str,
    status: TaskStatus,
    progress: f64,
) -> opca_core::lifecycle::Heartbeat {
    opca_core::lifecycle::Heartbeat {
        task_id: task_id.to_string(),
        status,
        progress,
        summary: format!("{status:?} at {progress:.0}%"),
        timestamp: 0,
        todo: None,
        subtasks: Vec::new(),
    }
}

async fn wait_for_heartbeat(orch: &mut Orchestrator, task_id: &str, timeout_ms: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        orch.drain_heartbeats();
        if orch.latest_heartbeat(task_id).is_some() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for heartbeat for {task_id}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

// ── Task 10.3: routing ──────────────────────────────────────────────────────

#[test]
fn routing_quick_question_foreground() {
    let cases = [
        "what does this function do?",
        "how does the auth flow work?",
        "why is the build failing?",
        "explain the data model",
        "where is the config file?",
        "show me the API endpoints",
    ];
    for msg in &cases {
        let decision = route(msg, "");
        assert_eq!(
            decision,
            RouteDecision::Foreground,
            "expected Foreground for: {msg}"
        );
    }
}

#[test]
fn routing_long_task_background() {
    let cases = [
        "refactor the auth module to OAuth2",
        "implement user registration",
        "fix the login bug",
        "rewrite the data layer",
        "create a new API endpoint",
        "build the migration script",
        "add input validation",
        "migrate from REST to GraphQL",
        "optimize the database queries",
        "delete the legacy code",
    ];
    for msg in &cases {
        let decision = route(msg, "");
        assert!(
            matches!(decision, RouteDecision::Background { .. }),
            "expected Background for: {msg}, got {decision:?}"
        );
    }
}

#[test]
fn routing_background_carries_description() {
    let decision = route("refactor the auth module", "");
    match decision {
        RouteDecision::Background { description, .. } => {
            assert_eq!(description, "refactor the auth module");
        }
        RouteDecision::Foreground => panic!("expected Background"),
    }
}

#[test]
fn routing_plain_greeting_foreground() {
    let decision = route("hello", "");
    assert_eq!(decision, RouteDecision::Foreground);
}

#[test]
fn routing_question_overrides_action_word() {
    let decision = route("how should I implement the fix?", "");
    assert_eq!(decision, RouteDecision::Foreground);
}

// ── Task 10.5: dispatched Task receives correct focus dimensions ─────────────

#[tokio::test]
async fn dispatch_task_stores_focus_dimensions() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("ok").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let focus = vec![
        "security risks".to_string(),
        "breaking changes".to_string(),
        "tradeoff decisions".to_string(),
    ];
    let task_id = orch
        .dispatch_task("refactor auth", focus.clone(), vec![], None)
        .await
        .expect("dispatch");

    let stored_focus = orch.task_focus(&task_id).expect("task should exist");
    for dim in &focus {
        assert!(
            stored_focus.contains(dim),
            "focus should contain '{dim}', got: {:?}",
            stored_focus.dimensions()
        );
    }
    assert_eq!(stored_focus.dimensions().len(), focus.len());
}

#[tokio::test]
async fn dispatch_task_returns_unique_ids() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let id1 = orch
        .dispatch_task("task one", vec![], vec![], None)
        .await
        .unwrap();
    let id2 = orch
        .dispatch_task("task two", vec![], vec![], None)
        .await
        .unwrap();

    assert_ne!(id1, id2);
}

#[tokio::test]
async fn dispatch_task_registers_in_registry() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("ok").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    assert_eq!(orch.task_count(), 0);
    let _id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    assert_eq!(orch.task_count(), 1);
}

#[tokio::test]
async fn dispatch_task_is_marked_dispatched() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("ok").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    assert!(orch.is_dispatched(&task_id));
}

// ── Task 10.7: heartbeat updates registry ────────────────────────────────────

#[tokio::test]
async fn heartbeat_updates_registry() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("ok").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    wait_for_heartbeat(&mut orch, &task_id, 3000).await;

    let hb = orch
        .latest_heartbeat(&task_id)
        .expect("should have heartbeat");
    assert!(
        !hb.summary.is_empty(),
        "heartbeat summary should not be empty"
    );
}

#[tokio::test]
async fn heartbeat_simulated_updates_registry() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();

    let hb = make_heartbeat(&task_id, TaskStatus::OnIt, 0.5);
    orch.heartbeat_sender().send((task_id.clone(), hb)).unwrap();
    orch.drain_heartbeats();

    let latest = orch
        .latest_heartbeat(&task_id)
        .expect("should have heartbeat");
    assert_eq!(latest.status, TaskStatus::OnIt);
    assert!((latest.progress - 0.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn latest_heartbeat_reflects_most_recent() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();

    orch.heartbeat_sender()
        .send((
            task_id.clone(),
            make_heartbeat(&task_id, TaskStatus::Waking, 0.0),
        ))
        .unwrap();
    orch.heartbeat_sender()
        .send((
            task_id.clone(),
            make_heartbeat(&task_id, TaskStatus::Pondering, 0.1),
        ))
        .unwrap();
    orch.heartbeat_sender()
        .send((
            task_id.clone(),
            make_heartbeat(&task_id, TaskStatus::OnIt, 0.5),
        ))
        .unwrap();
    orch.drain_heartbeats();

    let latest = orch.latest_heartbeat(&task_id).unwrap();
    assert_eq!(latest.status, TaskStatus::OnIt);
    assert!((latest.progress - 0.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn heartbeat_none_for_unknown_task() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    assert!(orch.latest_heartbeat("nonexistent").is_none());
}

// ── Task 10.8: deep_dive ────────────────────────────────────────────────────

#[tokio::test]
async fn deep_dive_returns_filtered_messages() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    orch.set_context_snapshot(
        &task_id,
        vec![
            Message::user("refactor the auth module"),
            Message::assistant("looking at auth.rs"),
            Message::user("also check the utils"),
            Message::assistant("utils.rs is fine"),
        ],
    )
    .unwrap();

    let results = orch.deep_dive(&task_id, "auth").unwrap();
    assert_eq!(results.len(), 2, "should return messages containing 'auth'");
    assert!(results.iter().all(|m| m.content.contains("auth")));
}

#[tokio::test]
async fn deep_dive_empty_query_returns_all() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    orch.set_context_snapshot(
        &task_id,
        vec![
            Message::user("message one"),
            Message::assistant("response one"),
        ],
    )
    .unwrap();

    let results = orch.deep_dive(&task_id, "").unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn deep_dive_unknown_task_errors() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let result = orch.deep_dive("nonexistent", "query");
    assert!(result.is_err());
}

// ── Task 10.9 / 10.11: recall by keyword/time/task_id/tag ────────────────────

#[test]
fn recall_by_keyword() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let e1 = OrchestratorEvent::new(
        EventKind::Other,
        "task-A".to_string(),
        "auth refactor summary".to_string(),
    );
    let e2 = OrchestratorEvent::new(
        EventKind::Other,
        "task-B".to_string(),
        "network optimization".to_string(),
    );
    orch.remember(&e1, &[]).unwrap();
    orch.remember(&e2, &[]).unwrap();

    let hits = orch.recall(&RecallQuery::Keyword("auth".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "auth refactor summary");

    let hits = orch
        .recall(&RecallQuery::Keyword("network".into()))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].task_id, "task-B");
}

#[test]
fn recall_by_tag() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let e1 = OrchestratorEvent::new(
        EventKind::Heartbeat,
        "task-A".to_string(),
        "auth work".to_string(),
    );
    orch.remember(&e1, &["security", "auth"]).unwrap();

    let hits = orch.recall(&RecallQuery::Tag("security".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].task_id, "task-A");

    let hits = orch.recall(&RecallQuery::Tag("auth".into())).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn recall_by_task_id() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let e1 = OrchestratorEvent::new(
        EventKind::Heartbeat,
        "task-A".to_string(),
        "heartbeat 1".to_string(),
    );
    let e2 = OrchestratorEvent::new(
        EventKind::Heartbeat,
        "task-B".to_string(),
        "heartbeat 2".to_string(),
    );
    orch.remember(&e1, &[]).unwrap();
    orch.remember(&e2, &[]).unwrap();

    let hits = orch.recall(&RecallQuery::TaskId("task-A".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "heartbeat 1");
}

#[test]
fn recall_by_time_range() {
    let fake_clock = Arc::new(opca_test_utils::FakeClock::new(UNIX_EPOCH));
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let (orch, _tmp) = make_orchestrator(provider, fake_clock.clone());

    let e1 = OrchestratorEvent::new(
        EventKind::Other,
        "task-A".to_string(),
        "old event".to_string(),
    );
    orch.remember(&e1, &[]).unwrap();

    fake_clock.advance(Duration::from_secs(100));

    let e2 = OrchestratorEvent::new(
        EventKind::Other,
        "task-B".to_string(),
        "new event".to_string(),
    );
    orch.remember(&e2, &[]).unwrap();

    let t0 = UNIX_EPOCH;
    let t_mid = UNIX_EPOCH + Duration::from_secs(50);
    let t_end = UNIX_EPOCH + Duration::from_secs(200);

    let only_old = orch
        .recall(&RecallQuery::TimeRange {
            from: t0,
            to: t_mid,
        })
        .unwrap();
    assert_eq!(only_old.len(), 1);
    assert_eq!(only_old[0].text, "old event");

    let both = orch
        .recall(&RecallQuery::TimeRange {
            from: t0,
            to: t_end,
        })
        .unwrap();
    assert_eq!(both.len(), 2);
}

#[test]
fn recall_no_match_returns_empty() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let hits = orch
        .recall(&RecallQuery::Keyword("nonexistent_term".into()))
        .unwrap();
    assert!(hits.is_empty());
}

// ── Task 10.10: prefetch ─────────────────────────────────────────────────────

#[test]
fn prefetch_caches_recall_results() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let e = OrchestratorEvent::new(
        EventKind::Other,
        "task-A".to_string(),
        "auth security finding".to_string(),
    );
    orch.remember(&e, &["security"]).unwrap();

    orch.on_user_message("tell me about auth");

    let cache = orch.prefetch_cache();
    assert!(
        !cache.is_empty(),
        "prefetch cache should contain recalled items"
    );
    assert!(cache.iter().any(|e| e.text.contains("auth")));
}

#[test]
fn prefetch_empty_when_no_match() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    orch.on_user_message("something unrelated");

    let cache = orch.prefetch_cache();
    assert!(cache.is_empty());
}

// ── Task 10.12 / 10.13: conflict prediction ──────────────────────────────────

#[test]
fn predict_conflict_overlapping() {
    assert!(predict_conflict(
        &[PathBuf::from("src/auth.rs")],
        &[PathBuf::from("src/auth.rs")]
    ));
}

#[test]
fn predict_conflict_non_overlapping() {
    assert!(!predict_conflict(
        &[PathBuf::from("src/auth.rs")],
        &[PathBuf::from("src/utils.rs")]
    ));
}

#[test]
fn predict_conflict_empty_no_conflict() {
    assert!(!predict_conflict(&[], &[PathBuf::from("src/auth.rs")]));
    assert!(!predict_conflict(&[PathBuf::from("src/auth.rs")], &[]));
}

#[tokio::test]
async fn non_overlapping_tasks_both_dispatched() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let id_a = orch
        .dispatch_task(
            "refactor auth",
            vec![],
            vec![PathBuf::from("src/auth.rs")],
            None,
        )
        .await
        .unwrap();
    let id_b = orch
        .dispatch_task(
            "refactor utils",
            vec![],
            vec![PathBuf::from("src/utils.rs")],
            None,
        )
        .await
        .unwrap();

    assert!(orch.is_dispatched(&id_a), "Task A should be dispatched");
    assert!(orch.is_dispatched(&id_b), "Task B should be dispatched");
}

#[tokio::test]
async fn overlapping_tasks_second_queued() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let id_a = orch
        .dispatch_task(
            "refactor auth",
            vec![],
            vec![PathBuf::from("src/auth.rs")],
            None,
        )
        .await
        .unwrap();
    let id_c = orch
        .dispatch_task(
            "fix auth bug",
            vec![],
            vec![PathBuf::from("src/auth.rs")],
            None,
        )
        .await
        .unwrap();

    assert!(orch.is_dispatched(&id_a), "Task A should be dispatched");
    assert!(
        !orch.is_dispatched(&id_c),
        "Task C should be queued (conflict with A)"
    );
}

#[tokio::test]
async fn queued_task_still_registered() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let id_a = orch
        .dispatch_task("task A", vec![], vec![PathBuf::from("src/main.rs")], None)
        .await
        .unwrap();
    let _id_b = orch
        .dispatch_task("task B", vec![], vec![PathBuf::from("src/main.rs")], None)
        .await
        .unwrap();

    assert_eq!(orch.task_count(), 2, "both tasks should be registered");
    assert!(orch.task_focus(&id_a).is_some());
}

// ── Task 10.14: update_focus ─────────────────────────────────────────────────

#[tokio::test]
async fn update_focus_sends_steering_message() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();

    let update = FocusUpdate::new()
        .with_add(vec!["performance".to_string()])
        .with_reason("user requested");

    let result = orch.update_focus(&task_id, update);
    assert!(
        result.is_ok(),
        "update_focus should succeed for dispatched task"
    );
}

#[tokio::test]
async fn update_focus_unknown_task_errors() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);

    let update = FocusUpdate::new().with_add(vec!["performance".to_string()]);
    let result = orch.update_focus("nonexistent", update);
    assert!(result.is_err());
}

#[tokio::test]
async fn update_focus_queued_task_fails() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let _id_a = orch
        .dispatch_task("task A", vec![], vec![PathBuf::from("src/x.rs")], None)
        .await
        .unwrap();
    let id_b = orch
        .dispatch_task("task B", vec![], vec![PathBuf::from("src/x.rs")], None)
        .await
        .unwrap();

    let update = FocusUpdate::new().with_add(vec!["security".to_string()]);
    let result = orch.update_focus(&id_b, update);
    assert!(
        result.is_err(),
        "update_focus should fail for queued task (steering channel closed)"
    );
}

// ── Integration: full dispatch + heartbeat + deep_dive flow ───────────────────

#[tokio::test]
async fn full_flow_dispatch_heartbeat_and_query() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("done").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let focus = vec!["security risks".to_string(), "breaking changes".to_string()];
    let task_id = orch
        .dispatch_task("refactor auth module", focus, vec![], None)
        .await
        .unwrap();

    wait_for_heartbeat(&mut orch, &task_id, 5000).await;

    let hb = orch.latest_heartbeat(&task_id);
    assert!(hb.is_some(), "should have heartbeat after dispatch");

    let stored_focus = orch.task_focus(&task_id).unwrap();
    assert!(stored_focus.contains("security risks"));
    assert!(stored_focus.contains("breaking changes"));
}

// ── Orchestrator Debug ────────────────────────────────────────────────────────

#[test]
fn orchestrator_debug_does_not_panic() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (orch, _tmp) = make_orchestrator(provider, clock);
    let debug_str = format!("{orch:?}");
    assert!(debug_str.contains("Orchestrator"));
}

// ── Highlight aggregation ─────────────────────────────────────────────────────

#[tokio::test]
async fn highlight_aggregation() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec!["security".to_string()], vec![], None)
        .await
        .unwrap();

    let hl = Highlight::new("security", Severity::Warning, "found SQL injection");
    orch.highlight_sender().send((task_id.clone(), hl)).unwrap();
    orch.drain_highlights();

    let highlights = orch.task_highlights(&task_id).unwrap();
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].tag, "security");
    assert_eq!(highlights[0].severity, Severity::Warning);
}

// ── Cancel and inject ────────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_task_sends_steering() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    let result = orch.cancel_task(&task_id);
    assert!(result.is_ok());
}

#[tokio::test]
async fn inject_message_sends_steering() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let (mut orch, _tmp) = make_orchestrator(provider, clock);

    let task_id = orch
        .dispatch_task("test", vec![], vec![], None)
        .await
        .unwrap();
    let result = orch.inject_message(&task_id, Message::user("additional info"));
    assert!(result.is_ok());
}
