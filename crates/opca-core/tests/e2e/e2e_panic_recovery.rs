//! Task 17.10 — E2E: Task panic → Crashed state → notification → workspace recoverable.
//!
//! Uses `spawn_task` with a future that panics, verifies a `TaskPanic` is
//! returned, and confirms a workspace created before the panic remains
//! accessible and usable for recovery.

use opca_core::lifecycle::{TaskPanic, spawn_task};
use opca_core::workspace::{CopyWorkspace, Workspace};

#[tokio::test]
#[ignore = "E2E: panic recovery with real workspace"]
async fn e2e_task_panic_workspace_recoverable() {
    let project = tempfile::tempdir().expect("tempdir");
    std::fs::write(project.path().join("data.txt"), b"important data").expect("write");
    let parent = tempfile::tempdir().expect("tempdir");

    let mut workspace = CopyWorkspace::create(project.path(), parent.path(), "panic-recovery")
        .expect("create workspace");

    std::fs::write(workspace.path().join("wip.txt"), b"work in progress").expect("write");
    let ws_path = workspace.path().to_path_buf();

    let result: Result<String, TaskPanic> = spawn_task("e2e-panic", async {
        panic!("simulated agent crash");
    })
    .await;

    let panic_err = result.expect_err("should get TaskPanic");
    assert_eq!(panic_err.task_id, "e2e-panic");
    assert!(
        panic_err.message.contains("simulated agent crash"),
        "panic message should be preserved, got: {}",
        panic_err.message
    );

    let display = format!("{panic_err}");
    assert!(display.contains("e2e-panic"));
    assert!(display.contains("simulated agent crash"));

    assert!(
        ws_path.exists(),
        "workspace directory should still exist after panic"
    );
    assert!(
        ws_path.join("wip.txt").exists(),
        "workspace files should be intact after panic"
    );
    assert!(
        ws_path.join("data.txt").exists(),
        "project files should be intact in workspace"
    );

    let diff = workspace.diff().expect("diff");
    assert!(
        !diff.is_empty(),
        "workspace should still report changes after panic"
    );

    std::fs::write(ws_path.join("recovered.txt"), b"recovery write").expect("write");
    assert!(
        ws_path.join("recovered.txt").exists(),
        "workspace should accept writes after panic recovery"
    );

    workspace.freeze().expect("freeze should work after panic");
    assert!(workspace.is_frozen());
}
