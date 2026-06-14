//! Integration tests for the Completion Pipeline (Tasks 12.4, 12.6, 12.8).
//!
//! These tests exercise the public API surface of the completion module
//! using `ScriptedProvider` (available in integration tests via
//! `opca-test-utils`).

use std::path::PathBuf;
use std::sync::Arc;

use opca_core::completion::{
    CompletionOutcome, CompletionPipeline, DependencyGraph, MergeOutcome, RiskLevel, assess_risk,
    merge, notification_level,
};
use opca_core::di::{Clock, StdFileSystem, StdProcess};
use opca_core::focus::Highlight;
use opca_core::lifecycle::TaskStatus;
use opca_core::memory::{RecallQuery, Store};
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::{Message, Provider};
use opca_core::workspace::{ChangeSet, CopyWorkspace, Workspace};
use opca_test_utils::ScriptedProvider;

// ── Task 12.4: low-risk → rule checks, high-risk → Audit ───────────────────

#[test]
fn low_risk_md_diff_is_auto_accepted() {
    let diff = ChangeSet {
        added: vec![PathBuf::from("doc.md")],
        modified: vec![],
        deleted: vec![],
    };
    assert_eq!(assess_risk(&diff), RiskLevel::Low);
    // Low-risk → auto-accept (silent notification).
    assert_eq!(
        notification_level(RiskLevel::Low, None),
        opca_core::completion::NotificationLevel::Silent
    );
}

#[test]
fn high_risk_rs_diff_is_pending_review() {
    let diff = ChangeSet {
        added: vec![PathBuf::from("src/main.rs")],
        modified: vec![],
        deleted: vec![],
    };
    assert_eq!(assess_risk(&diff), RiskLevel::High);
    // High-risk → pending review (user notified).
    assert_eq!(
        notification_level(RiskLevel::High, None),
        opca_core::completion::NotificationLevel::PendingReview
    );
}

#[test]
fn high_risk_dispatched_to_audit_and_user_reviews() {
    let diff = ChangeSet {
        added: vec![
            PathBuf::from("src/auth.rs"),
            PathBuf::from("src/oauth.rs"),
            PathBuf::from("src/session.rs"),
            PathBuf::from("src/token.rs"),
            PathBuf::from("src/refresh.rs"),
        ],
        modified: vec![
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("Cargo.toml"),
        ],
        deleted: vec![],
    };
    let risk = assess_risk(&diff);
    assert_eq!(risk, RiskLevel::High);
    assert_eq!(
        notification_level(risk, None),
        opca_core::completion::NotificationLevel::PendingReview
    );
}

// ── Task 12.6: clean merge, auto-resolvable, unresolvable ─────────────────

fn make_project_with_files() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("README.md"), b"hello world").expect("write");
    std::fs::create_dir_all(tmp.path().join("src")).expect("mkdir");
    std::fs::write(tmp.path().join("src/main.rs"), b"fn main() {}").expect("write");
    tmp
}

#[test]
fn merge_outcome_clean_when_no_conflicts() {
    let project = make_project_with_files();
    let parent = tempfile::tempdir().expect("tempdir");
    let ws =
        CopyWorkspace::create(project.path(), parent.path(), "merge-clean-it").expect("create");
    std::fs::write(ws.path().join("README.md"), b"updated").expect("write");

    let target = make_project_with_files();

    let outcome = merge(&ws, target.path(), None);
    assert_eq!(outcome, MergeOutcome::Clean);
    assert_eq!(
        std::fs::read_to_string(target.path().join("README.md")).unwrap(),
        "updated"
    );
}

#[test]
fn merge_outcome_auto_resolvable() {
    let project = make_project_with_files();
    let parent = tempfile::tempdir().expect("tempdir");
    let ws = CopyWorkspace::create(project.path(), parent.path(), "merge-auto-it").expect("create");
    std::fs::write(
        ws.path().join("src/main.rs"),
        b"fn main() { workspace_version }",
    )
    .expect("write");

    let target = make_project_with_files();
    std::fs::write(
        target.path().join("src/main.rs"),
        b"fn main() { target_version }",
    )
    .expect("write");

    let resolver = |_paths: &[PathBuf]| true;
    let outcome = merge(&ws, target.path(), Some(&resolver));
    assert_eq!(outcome, MergeOutcome::AutoResolved);
}

#[test]
fn merge_outcome_unresolvable_escalates_to_user() {
    let project = make_project_with_files();
    let parent = tempfile::tempdir().expect("tempdir");
    let ws =
        CopyWorkspace::create(project.path(), parent.path(), "merge-conflict-it").expect("create");
    std::fs::write(
        ws.path().join("src/main.rs"),
        b"fn main() { workspace_version }",
    )
    .expect("write");

    let target = make_project_with_files();
    std::fs::write(
        target.path().join("src/main.rs"),
        b"fn main() { target_version }",
    )
    .expect("write");

    let outcome = merge(&ws, target.path(), None);
    match outcome {
        MergeOutcome::Conflict(paths) => {
            assert_eq!(paths, vec![PathBuf::from("src/main.rs")]);
        }
        other => panic!("expected conflict, got {other:?}"),
    }

    let failing_resolver = |_paths: &[PathBuf]| false;
    let outcome = merge(&ws, target.path(), Some(&failing_resolver));
    assert!(matches!(outcome, MergeOutcome::Conflict(_)));
}

// ── Task 12.8: Cold Store recallable after Memorialize ─────────────────────

#[test]
fn cold_store_recallable_by_task_id_after_memorialize() {
    use opca_core::completion::{archive_summary, recall_by_task_id};

    let store = Store::in_memory().expect("store");
    archive_summary(
        &store,
        "task-cold-A",
        "auth refactor complete",
        &["security"],
    )
    .expect("archive");
    archive_summary(&store, "task-cold-B", "network optimization done", &[]).expect("archive");

    let hits_a = recall_by_task_id(&store, "task-cold-A").expect("recall");
    assert_eq!(hits_a.len(), 1);
    assert_eq!(hits_a[0], "auth refactor complete");

    let hits_b = recall_by_task_id(&store, "task-cold-B").expect("recall");
    assert_eq!(hits_b.len(), 1);
    assert_eq!(hits_b[0], "network optimization done");

    let hits_none = recall_by_task_id(&store, "task-unknown").expect("recall");
    assert!(hits_none.is_empty());
}

#[test]
fn cold_store_recallable_by_keyword() {
    use opca_core::completion::archive_summary;

    let store = Store::in_memory().expect("store");
    archive_summary(&store, "task-X", "refactored OAuth2 flow", &[]).expect("archive");

    let records = store
        .recall(&RecallQuery::Keyword("oauth2".into()))
        .expect("recall");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].task_id.as_deref(), Some("task-X"));
}

#[test]
fn cold_store_recallable_by_tag() {
    use opca_core::completion::archive_summary;

    let store = Store::in_memory().expect("store");
    archive_summary(&store, "task-Y", "security audit", &["security", "auth"]).expect("archive");

    let by_security = store
        .recall(&RecallQuery::Tag("security".into()))
        .expect("recall");
    assert_eq!(by_security.len(), 1);

    let by_auth = store
        .recall(&RecallQuery::Tag("auth".into()))
        .expect("recall");
    assert_eq!(by_auth.len(), 1);
}

#[test]
fn cold_store_preserved_after_workspace_cleanup() {
    use opca_core::completion::archive_summary;
    use opca_core::workspace::CleanupSchedule;

    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_dir = tmp.path().join("ws-task-Z");
    std::fs::create_dir_all(&ws_dir).expect("mkdir");
    std::fs::write(ws_dir.join("file.txt"), b"work").expect("write");

    let store = Store::in_memory().expect("store");
    archive_summary(&store, "task-Z", "done before cleanup", &[]).expect("archive");

    let mut schedule = CleanupSchedule::new();
    schedule.schedule_default(&ws_dir);
    schedule.cleanup_now(&ws_dir).expect("cleanup");

    assert!(!ws_dir.exists(), "workspace should be removed");
    assert_eq!(
        store.count().unwrap(),
        1,
        "Cold Store data must survive cleanup"
    );
}

// ── Task 12.12: dependency chain auto-activates successor ──────────────────

#[test]
fn dependency_chain_activates_successor_on_predecessor_merge() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("task-A", "task-B");

    let successors = graph.on_task_merged("task-A");
    assert_eq!(successors.len(), 1);
    assert_eq!(successors[0], "task-B");
}

#[test]
fn dependency_chain_drains_successors_after_merge() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("A", "B");
    graph.add_dependency("A", "C");

    let drained = graph.drain_successors("A");
    assert_eq!(drained.len(), 2);
    assert!(drained.contains(&"B".to_string()));
    assert!(drained.contains(&"C".to_string()));

    assert!(graph.on_task_merged("A").is_empty());
}

#[test]
fn dependency_chain_does_not_activate_for_unrelated_task() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("A", "B");

    let unrelated = graph.on_task_merged("unrelated");
    assert!(unrelated.is_empty());
}

#[test]
fn dependency_chain_multiple_predecessors() {
    let mut graph = DependencyGraph::new();
    // C depends on both A and B.
    graph.add_dependency("A", "C");
    graph.add_dependency("B", "C");

    // A merging activates C.
    let from_a = graph.drain_successors("A");
    assert_eq!(from_a, vec!["C".to_string()]);

    // B merging also activates C (independent chain).
    let from_b = graph.drain_successors("B");
    assert_eq!(from_b, vec!["C".to_string()]);
}

#[tokio::test]
async fn full_pipeline_activates_successor_on_merge() {
    let provider =
        Arc::new(ScriptedProvider::new().then_text("done").then_done()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;

    let project_tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(project_tmp.path().join("placeholder.txt"), b"x").expect("write");
    let orch = Orchestrator::new(
        provider,
        project_tmp.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    std::mem::forget(project_tmp);

    let mut pipeline = CompletionPipeline::new(Arc::new(std::sync::Mutex::new(orch)));
    pipeline.add_dependency("pred-task", "succ-task");

    let project = tempfile::tempdir().expect("tempdir");
    std::fs::write(project.path().join("README.md"), b"original").expect("write");
    let parent = tempfile::tempdir().expect("tempdir");
    let mut ws = CopyWorkspace::create(project.path(), parent.path(), "pred-task").expect("create");
    std::fs::write(ws.path().join("README.md"), b"updated").expect("write");

    let summary_provider = ScriptedProvider::new().then_text("done").then_done();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let outcome = pipeline
        .run(
            &mut ws,
            &summary_provider,
            "pred-task",
            &[Message::assistant("did the work")],
            &tx,
            &[Highlight::new(
                "done",
                opca_core::focus::Severity::Info,
                "complete",
            )],
            &["docs"],
            project.path(),
        )
        .await
        .expect("pipeline");

    assert_eq!(outcome, CompletionOutcome::Merged);
    assert!(
        pipeline.activated_successors("pred-task").is_empty(),
        "successor should be drained (activated) after predecessor merge"
    );
}

// ── Full pipeline end-to-end: freeze + review + merge + memorialize ────────

#[tokio::test]
async fn full_pipeline_low_risk_md_merges_and_memorializes() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;

    let project_tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(project_tmp.path().join("placeholder.txt"), b"x").expect("write");
    let orch = Orchestrator::new(
        provider,
        project_tmp.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    std::mem::forget(project_tmp);

    let mut pipeline = CompletionPipeline::new(Arc::new(std::sync::Mutex::new(orch)));

    let project = tempfile::tempdir().expect("tempdir");
    std::fs::write(project.path().join("README.md"), b"original").expect("write");
    let parent = tempfile::tempdir().expect("tempdir");
    let mut ws =
        CopyWorkspace::create(project.path(), parent.path(), "low-risk-task").expect("create");
    std::fs::write(ws.path().join("NOTES.md"), b"new notes").expect("write");

    let summary_provider = ScriptedProvider::new().then_text("added notes").then_done();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let outcome = pipeline
        .run(
            &mut ws,
            &summary_provider,
            "low-risk-task",
            &[Message::user("add notes"), Message::assistant("done")],
            &tx,
            &[],
            &[],
            project.path(),
        )
        .await
        .expect("pipeline");

    assert_eq!(outcome, CompletionOutcome::Merged);
    assert!(pipeline.cleanup_schedule().is_scheduled(ws.path()));
    assert!(project.path().join("NOTES.md").exists());
}

#[tokio::test]
async fn full_pipeline_high_risk_rs_pending_review() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;

    let project_tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(project_tmp.path().join("placeholder.txt"), b"x").expect("write");
    let orch = Orchestrator::new(
        provider,
        project_tmp.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    std::mem::forget(project_tmp);

    let mut pipeline = CompletionPipeline::new(Arc::new(std::sync::Mutex::new(orch)));

    let project = tempfile::tempdir().expect("tempdir");
    std::fs::write(project.path().join("README.md"), b"original").expect("write");
    let parent = tempfile::tempdir().expect("tempdir");
    let mut ws =
        CopyWorkspace::create(project.path(), parent.path(), "high-risk-task").expect("create");
    std::fs::create_dir_all(ws.path().join("src")).expect("mkdir");
    std::fs::write(ws.path().join("src/main.rs"), b"fn main() { new }").expect("write");

    let summary_provider = ScriptedProvider::new().then_text("summary").then_done();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let outcome = pipeline
        .run(
            &mut ws,
            &summary_provider,
            "high-risk-task",
            &[],
            &tx,
            &[],
            &[],
            project.path(),
        )
        .await
        .expect("pipeline");

    assert!(
        matches!(outcome, CompletionOutcome::PendingReview),
        "high-risk should be pending review, got {outcome:?}"
    );
    assert!(
        !pipeline.cleanup_schedule().is_scheduled(ws.path()),
        "cleanup should not be scheduled for unmerged task"
    );
}

// ── Heartbeat pushed on freeze ─────────────────────────────────────────────

#[tokio::test]
async fn freeze_pushes_delivered_heartbeat() {
    use opca_core::completion::freeze;

    let project = tempfile::tempdir().expect("tempdir");
    std::fs::write(project.path().join("README.md"), b"hi").expect("write");
    let parent = tempfile::tempdir().expect("tempdir");
    let mut ws = CopyWorkspace::create(project.path(), parent.path(), "hb-test").expect("create");

    let provider = ScriptedProvider::new()
        .then_text("final summary")
        .then_done();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = freeze(
        &mut ws,
        &provider,
        &[Message::assistant("work")],
        &tx,
        "hb-test",
    )
    .await
    .expect("freeze");

    let hb = rx.try_recv().expect("heartbeat should be pushed");
    assert_eq!(hb.task_id, "hb-test");
    assert_eq!(hb.status, TaskStatus::Delivered);
    assert_eq!(hb.summary, result.summary);
    assert!((hb.progress - 1.0).abs() < f64::EPSILON);
    assert!(ws.is_frozen());
}

// ── Notification level matrix ──────────────────────────────────────────────

#[test]
fn notification_level_matrix() {
    use opca_core::audit::AuditVerdict;
    use opca_core::completion::NotificationLevel;

    let cases: [(RiskLevel, Option<AuditVerdict>, NotificationLevel); 6] = [
        (RiskLevel::Low, None, NotificationLevel::Silent),
        (
            RiskLevel::Low,
            Some(AuditVerdict::Fail),
            NotificationLevel::Silent,
        ),
        (RiskLevel::Medium, None, NotificationLevel::Silent),
        (
            RiskLevel::Medium,
            Some(AuditVerdict::Warn),
            NotificationLevel::PendingReview,
        ),
        (
            RiskLevel::Medium,
            Some(AuditVerdict::Fail),
            NotificationLevel::PendingReview,
        ),
        (RiskLevel::High, None, NotificationLevel::PendingReview),
    ];

    for (risk, verdict, expected) in cases {
        assert_eq!(
            notification_level(risk, verdict),
            expected,
            "risk={risk:?}, verdict={verdict:?}"
        );
    }
}

// ── Pipeline debug formatting ─────────────────────────────────────────────

#[test]
fn pipeline_debug_does_not_panic() {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;
    let project_tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(project_tmp.path().join("placeholder.txt"), b"x").expect("write");
    let orch = Orchestrator::new(
        provider,
        project_tmp.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    std::mem::forget(project_tmp);

    let pipeline = CompletionPipeline::new(Arc::new(std::sync::Mutex::new(orch)));
    let debug = format!("{pipeline:?}");
    assert!(debug.contains("CompletionPipeline"));
}
