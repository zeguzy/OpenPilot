//! Task 17.5 — E2E: focus contract dynamic update mid-Task.
//!
//! Creates a Task with an initial `FocusContract`, sends a `FocusUpdate` via the
//! steering channel before the agent loop starts, runs the Task, and verifies
//! the contract was modified and the new dimension is reportable.

use std::sync::Arc;

use opca_core::di::Clock;
use opca_core::focus::{FocusContract, FocusUpdate};
use opca_core::task::{SteeringMessage, Task, TaskOutcome};
use opca_core::tools::{ToolContext, ToolRegistry};
use opca_core::workspace::{MergeResult, Result as WsResult, Workspace};
use opca_test_utils::{FakeClock, MockFileSystem, ScriptedProvider};

struct StubWs;

impl Workspace for StubWs {
    fn path(&self) -> &std::path::Path {
        std::path::Path::new("/stub")
    }
    fn freeze(&mut self) -> WsResult<()> {
        Ok(())
    }
    fn diff(&self) -> WsResult<opca_core::workspace::ChangeSet> {
        Ok(opca_core::workspace::ChangeSet::default())
    }
    fn merge_into(&self, _target: &std::path::Path) -> WsResult<MergeResult> {
        Ok(MergeResult::Clean)
    }
    fn cleanup(&mut self) -> WsResult<()> {
        Ok(())
    }
    fn is_frozen(&self) -> bool {
        false
    }
}

#[tokio::test]
#[ignore = "E2E: focus contract dynamic update"]
async fn e2e_focus_update_mid_task() {
    let provider = ScriptedProvider::new().then_text("done").then_done();

    let mut focus = FocusContract::empty();
    focus.add("security risks").unwrap();

    let tools = ToolRegistry::new();
    let tool_ctx = ToolContext {
        workspace_path: std::path::PathBuf::from("/stub"),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(opca_test_utils::MockProcess::new()),
        task_id: None,
    };
    let clock = Arc::new(FakeClock::default()) as Arc<dyn Clock>;

    let (mut task, handle) = Task::new(
        "e2e-focus",
        Arc::new(provider) as Arc<dyn opca_core::provider::Provider>,
        Box::new(StubWs),
        focus,
        tools,
        tool_ctx,
        clock,
    );

    assert!(task.focus().contains("security risks"));
    assert!(!task.focus().contains("performance"));

    let update = FocusUpdate::new()
        .with_add(vec!["performance".to_string()])
        .with_remove(vec!["security risks".to_string()])
        .with_reason("user redirected focus to performance");

    handle
        .steering_tx
        .send(SteeringMessage::UpdateFocus(update))
        .expect("send steering");

    let outcome = task.run("do work").await;
    assert!(matches!(outcome, TaskOutcome::Completed(_)));

    assert!(
        !task.focus().contains("security risks"),
        "security risks should be removed"
    );
    assert!(
        task.focus().contains("performance"),
        "performance should be added"
    );
}
