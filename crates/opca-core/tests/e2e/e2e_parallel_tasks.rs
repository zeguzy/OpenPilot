//! Task 17.3 — E2E: two parallel Tasks with non-overlapping files.
//!
//! Creates a git project, dispatches two Tasks via the Orchestrator with
//! non-overlapping `estimated_files`, verifies both are dispatched (no
//! conflict), waits for both to reach `Delivered`.

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use opca_core::di::{Clock, StdFileSystem, StdProcess};
use opca_core::lifecycle::TaskStatus;
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::Provider;
use opca_test_utils::ScriptedProvider;

fn run_git(cwd: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_EMAIL", "opca@local")
        .env("GIT_AUTHOR_NAME", "opca")
        .env("GIT_COMMITTER_EMAIL", "opca@local")
        .env("GIT_COMMITTER_NAME", "opca")
        .status()
        .unwrap_or_else(|e| panic!("spawn git: {e}"));
    assert!(status.success(), "git {args:?} failed");
}

fn make_git_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("README.md"), b"parallel test").expect("write");
    run_git(root, &["init", "--quiet"]);
    let _ = Command::new("git")
        .args(["symbolic-ref", "HEAD", "refs/heads/main"])
        .current_dir(root)
        .status();
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-m", "init", "--quiet"]);
    tmp
}

async fn wait_for_delivered(orch: &mut Orchestrator, task_id: &str, timeout_ms: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        orch.drain_heartbeats();
        if let Some(hb) = orch.latest_heartbeat(task_id) {
            if hb.status == TaskStatus::Delivered {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for Delivered on {task_id}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
#[ignore = "E2E: requires real git CLI"]
async fn e2e_two_parallel_tasks_non_overlapping() {
    let project = make_git_project();

    let provider = Arc::new(
        ScriptedProvider::new()
            .then_text("task A done")
            .then_done()
            .then_text("task B done")
            .then_done(),
    ) as Arc<dyn Provider>;

    let clock = Arc::new(opca_test_utils::FakeClock::default()) as Arc<dyn Clock>;

    let mut orch = Orchestrator::new(
        provider,
        project.path().to_path_buf(),
        clock,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );

    let id_a = orch
        .dispatch_task(
            "task A: update docs",
            vec![],
            vec![std::path::PathBuf::from("docs.md")],
            None,
        )
        .await
        .expect("dispatch A");
    assert!(orch.is_dispatched(&id_a), "task A should be dispatched");

    let id_b = orch
        .dispatch_task(
            "task B: update config",
            vec![],
            vec![std::path::PathBuf::from("config.md")],
            None,
        )
        .await
        .expect("dispatch B");
    assert!(orch.is_dispatched(&id_b), "task B should be dispatched");
    assert_ne!(id_a, id_b, "task ids must be unique");
    assert_eq!(orch.task_count(), 2);

    wait_for_delivered(&mut orch, &id_a, 5000).await;
    wait_for_delivered(&mut orch, &id_b, 5000).await;

    let hb_a = orch.latest_heartbeat(&id_a).expect("heartbeat A");
    let hb_b = orch.latest_heartbeat(&id_b).expect("heartbeat B");
    assert_eq!(hb_a.status, TaskStatus::Delivered);
    assert_eq!(hb_b.status, TaskStatus::Delivered);
}
