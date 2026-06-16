//! Task 17.1 — E2E: dispatch → worktree → agent loop → completion → merge.
//!
//! Creates a real git project, creates a git-worktree workspace, simulates an
//! agent file change, runs the five-stage `CompletionPipeline`, and verifies the
//! change is merged back into the original project.

use std::process::Command;
use std::sync::{Arc, Mutex};

use opca_core::completion::{CompletionOutcome, CompletionPipeline};
use opca_core::di::{Clock, StdClock, StdFileSystem, StdProcess};
use opca_core::focus::FocusContract;
use opca_core::lifecycle::TaskStatus;
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::{Message, Provider};
use opca_core::task::{Task, TaskOutcome};
use opca_core::tools::{ToolContext, ToolRegistry};
use opca_core::workspace::WorkspaceManager;
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
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        cwd.display()
    );
}

fn make_git_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("README.md"), b"initial readme").expect("write");
    run_git(root, &["init", "--quiet"]);
    let _ = Command::new("git")
        .args(["symbolic-ref", "HEAD", "refs/heads/main"])
        .current_dir(root)
        .status();
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-m", "init", "--quiet"]);
    tmp
}

fn make_orchestrator(project: &std::path::Path) -> Arc<Mutex<Orchestrator>> {
    let provider = Arc::new(ScriptedProvider::new()) as Arc<dyn Provider>;
    let orch = Orchestrator::new(
        provider,
        project.to_path_buf(),
        Arc::new(StdClock) as Arc<dyn Clock>,
        Arc::new(StdFileSystem),
        Arc::new(StdProcess),
    );
    Arc::new(Mutex::new(orch))
}

#[tokio::test]
#[ignore = "E2E: requires real git CLI"]
async fn e2e_git_project_full_lifecycle() {
    let project = make_git_project();
    let parent = tempfile::tempdir().expect("tempdir");

    let mgr = WorkspaceManager::new().with_workspace_parent(parent.path());
    let workspace = mgr
        .create(project.path(), "e2e-git-1")
        .expect("workspace create");
    assert!(workspace.path().is_dir());
    assert!(workspace.path().join("README.md").exists());

    let git_marker = workspace.path().join(".git");
    assert!(git_marker.exists(), "git worktree should have .git marker");

    std::fs::write(workspace.path().join("NOTES.md"), b"agent added notes").expect("write");

    let provider =
        Arc::new(ScriptedProvider::new().then_text("done").then_done()) as Arc<dyn Provider>;
    let tool_ctx = ToolContext {
        workspace_path: workspace.path().to_path_buf(),
        fs: Arc::new(StdFileSystem),
        proc: Arc::new(StdProcess),
        task_id: None,
    };
    let (mut task, _handle) = Task::new(
        "e2e-git-1",
        provider.clone(),
        workspace,
        FocusContract::empty(),
        ToolRegistry::new(),
        tool_ctx,
        Arc::new(StdClock) as Arc<dyn Clock>,
    );

    let outcome = task.run("add notes").await;
    match outcome {
        TaskOutcome::Completed(msg) => assert_eq!(msg.text(), "done"),
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(task.lifecycle_current(), TaskStatus::Delivered);
    assert!(task.workspace().path().join("NOTES.md").exists());

    let freeze_provider = ScriptedProvider::new()
        .then_text("task summary")
        .then_done();
    let (hb_tx, _hb_rx) = tokio::sync::mpsc::unbounded_channel();
    let orch = make_orchestrator(project.path());
    let mut pipeline = CompletionPipeline::new(orch);

    let outcome = pipeline
        .run(
            task.workspace_mut(),
            &freeze_provider,
            "e2e-git-1",
            &[Message::user("add notes"), Message::assistant("done")],
            &hb_tx,
            &[],
            &[],
            project.path(),
        )
        .await
        .expect("pipeline run");

    match outcome {
        CompletionOutcome::Merged => {}
        other => panic!("expected Merged, got {other:?}"),
    }

    assert!(
        project.path().join("NOTES.md").exists(),
        "NOTES.md should be merged back into the project"
    );
}
