//! Task 17.2 — E2E: dispatch Task in non-git project → mirror → patch merge.
//!
//! Creates a non-git project, creates a `MirrorWorkspace` (internal git mirror),
//! simulates an agent file change, runs the `CompletionPipeline`, and verifies
//! the change is merged back into the original non-git project.

use std::sync::{Arc, Mutex};

use opca_core::completion::{CompletionOutcome, CompletionPipeline};
use opca_core::di::{Clock, StdClock, StdFileSystem, StdProcess};
use opca_core::orchestrator::Orchestrator;
use opca_core::provider::{Message, Provider};
use opca_core::workspace::{MIRROR_DIR_NAME, WorkspaceManager};
use opca_test_utils::ScriptedProvider;

fn make_plain_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("README.md"), b"plain project readme").expect("write");
    std::fs::write(root.join("guide.md"), b"some guide content").expect("write");
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
#[ignore = "E2E: requires real git CLI for internal mirror"]
async fn e2e_mirror_project_full_lifecycle() {
    let project = make_plain_project();
    let parent = tempfile::tempdir().expect("tempdir");

    let mgr = WorkspaceManager::new().with_workspace_parent(parent.path());
    let mut workspace = mgr
        .create(project.path(), "e2e-mirror-1")
        .expect("workspace create");
    assert!(workspace.path().is_dir());
    assert!(workspace.path().join("README.md").exists());

    assert!(
        project.path().join(MIRROR_DIR_NAME).exists(),
        "internal mirror repo should be created under .agent/mirror/"
    );

    std::fs::write(workspace.path().join("CHANGELOG.md"), b"v2 changes").expect("write");

    let freeze_provider = ScriptedProvider::new()
        .then_text("mirror task done")
        .then_done();
    let (hb_tx, _hb_rx) = tokio::sync::mpsc::unbounded_channel();
    let orch = make_orchestrator(project.path());
    let mut pipeline = CompletionPipeline::new(orch);

    let outcome = pipeline
        .run(
            &mut *workspace,
            &freeze_provider,
            "e2e-mirror-1",
            &[
                Message::user("add changelog"),
                Message::assistant("added CHANGELOG.md"),
            ],
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
        project.path().join("CHANGELOG.md").exists(),
        "CHANGELOG.md should be merged back into the non-git project"
    );

    let content =
        std::fs::read_to_string(project.path().join("CHANGELOG.md")).expect("read merged file");
    assert_eq!(content, "v2 changes");
}
