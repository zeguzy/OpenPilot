use std::path::{Path, PathBuf};
use std::sync::Arc;

use opca_core::focus::{FocusContract, FocusUpdate, Highlight, Severity};
use opca_core::lifecycle::TaskStatus;
use opca_core::provider::{Message, MessageRole, Provider};
use opca_core::task::{SteeringMessage, Task, TaskOutcome};
use opca_core::tools::{ToolContext, ToolRegistry};
use opca_core::workspace::{ChangeSet, MergeResult, Result as WsResult, Workspace};
use opca_test_utils::{FakeClock, MockFileSystem, MockProcess, ScriptedProvider};
use serde_json::json;

struct StubWorkspace {
    path: PathBuf,
}

impl StubWorkspace {
    fn new() -> Self {
        Self {
            path: PathBuf::from("/workspace"),
        }
    }
}

impl Workspace for StubWorkspace {
    fn path(&self) -> &Path {
        &self.path
    }
    fn freeze(&mut self) -> WsResult<()> {
        Ok(())
    }
    fn diff(&self) -> WsResult<ChangeSet> {
        Ok(ChangeSet::default())
    }
    fn merge_into(&self, _target: &Path) -> WsResult<MergeResult> {
        Ok(MergeResult::Clean)
    }
    fn cleanup(&mut self) -> WsResult<()> {
        Ok(())
    }
    fn is_frozen(&self) -> bool {
        false
    }
}

fn make_tool_ctx(fs: MockFileSystem) -> ToolContext {
    ToolContext {
        workspace_path: PathBuf::from("/workspace"),
        fs: Arc::new(fs),
        proc: Arc::new(MockProcess::new()),
    }
}

fn make_task(
    provider: ScriptedProvider,
    focus: FocusContract,
    fs: MockFileSystem,
) -> (Task, opca_core::task::TaskHandle) {
    let tools = ToolRegistry::new();
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    Task::new(
        "task-test",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        focus,
        tools,
        ctx,
        clock,
    )
}

fn drain_heartbeats(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<opca_core::lifecycle::Heartbeat>,
) -> Vec<opca_core::lifecycle::Heartbeat> {
    let mut hbs = Vec::new();
    while let Ok(hb) = rx.try_recv() {
        hbs.push(hb);
    }
    hbs
}

fn drain_highlights(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Highlight>) -> Vec<Highlight> {
    let mut hls = Vec::new();
    while let Ok(hl) = rx.try_recv() {
        hls.push(hl);
    }
    hls
}

fn drain_highlights_for_tag(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Highlight>,
    tag: &str,
) -> Vec<Highlight> {
    drain_highlights(rx)
        .into_iter()
        .filter(|h| h.tag == tag)
        .collect()
}

// ── Task 9.4: single-turn with ScriptedProvider ───────────────────────────

#[tokio::test]
async fn single_turn_text_response_completes() {
    let provider = ScriptedProvider::new().then_text("hello").then_done();
    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    let outcome = task.run("hi").await;

    match outcome {
        TaskOutcome::Completed(msg) => {
            assert_eq!(msg.content, "hello");
            assert!(msg.tool_calls.is_empty());
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn single_turn_active_messages_contain_exchange() {
    let provider = ScriptedProvider::new().then_text("hello").then_done();
    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("hi").await;

    let msgs = task.active_messages();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, MessageRole::User);
    assert_eq!(msgs[0].content, "hi");
    assert_eq!(msgs[1].role, MessageRole::Assistant);
    assert_eq!(msgs[1].content, "hello");
}

#[tokio::test]
async fn single_turn_final_status_is_delivered() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("go").await;
    assert_eq!(task.lifecycle_current(), TaskStatus::Delivered);
}

// ── Task 9.5: multi-turn with tool calls ───────────────────────────────────

#[tokio::test]
async fn multi_turn_tool_call_then_text_completes() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"file contents here");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("file contains X")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, _handle) = Task::new(
        "task-multi",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    let outcome = task.run("read foo.rs").await;

    match outcome {
        TaskOutcome::Completed(msg) => {
            assert_eq!(msg.content, "file contains X");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn multi_turn_active_messages_has_full_exchange() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"file contents here");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("file contains X")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, _handle) = Task::new(
        "task-multi",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    task.run("read foo.rs").await;

    let msgs = task.active_messages();
    // 1 user + 1 assistant(tool_call) + 1 tool_result + 1 assistant(text)
    assert_eq!(
        msgs.len(),
        4,
        "expected 4 messages, got {}: {msgs:?}",
        msgs.len()
    );

    assert_eq!(msgs[0].role, MessageRole::User);
    assert_eq!(msgs[0].content, "read foo.rs");

    assert_eq!(msgs[1].role, MessageRole::Assistant);
    assert_eq!(msgs[1].tool_calls.len(), 1);
    assert_eq!(msgs[1].tool_calls[0].name, "read");

    assert_eq!(msgs[2].role, MessageRole::Tool);
    assert!(msgs[2].tool_result.is_some());
    let result = msgs[2].tool_result.as_ref().unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("file contents here"));

    assert_eq!(msgs[3].role, MessageRole::Assistant);
    assert_eq!(msgs[3].content, "file contains X");
    assert!(msgs[3].tool_calls.is_empty());
}

#[tokio::test]
async fn multi_turn_turn_count_increments() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, _handle) = Task::new(
        "task-count",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    assert_eq!(task.turn_count(), 0);
    task.run("read foo.rs").await;
    assert_eq!(task.turn_count(), 2);
}

// ── Task 9.6: steering injects mid-loop ────────────────────────────────────

#[tokio::test]
async fn steering_inject_message_appears_in_context() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, handle) = Task::new(
        "task-steer",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    handle
        .steering_tx
        .send(SteeringMessage::Inject(Message::user("also check bar.rs")))
        .unwrap();

    task.run("read foo.rs").await;

    let has_injected = task
        .active_messages()
        .iter()
        .any(|m| m.content == "also check bar.rs");
    assert!(
        has_injected,
        "injected steering message should appear in active messages"
    );
}

#[tokio::test]
async fn steering_update_focus_modifies_contract() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let mut focus = FocusContract::empty();
    focus.add("security").unwrap();
    let (mut task, handle) = make_task(provider, focus, MockFileSystem::new());

    let update = FocusUpdate::new()
        .with_add(vec!["performance".to_string()])
        .with_remove(vec!["security".to_string()]);

    handle
        .steering_tx
        .send(SteeringMessage::UpdateFocus(update))
        .unwrap();

    task.run("go").await;

    assert!(!task.focus().contains("security"));
    assert!(task.focus().contains("performance"));
}

#[tokio::test]
async fn steering_cancel_terminates_loop() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let (mut task, handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    handle.steering_tx.send(SteeringMessage::Cancel).unwrap();

    let outcome = task.run("go").await;
    assert_eq!(outcome, TaskOutcome::Cancelled);
}

// ── Task 9.7: report_highlight pushes to Orchestrator ──────────────────────

#[tokio::test]
async fn report_highlight_pushes_highlight_to_channel() {
    let mut focus = FocusContract::empty();
    focus.add("security risks").unwrap();

    let provider = ScriptedProvider::new()
        .then_tool_call(
            "report_highlight",
            json!({
                "tag": "security risks",
                "severity": "warning",
                "summary": "hardcoded secret found"
            }),
        )
        .then_done()
        .then_text("reported")
        .then_done();

    let (mut task, mut handle) = make_task(provider, focus, MockFileSystem::new());

    let outcome = task.run("scan for issues").await;
    assert!(matches!(outcome, TaskOutcome::Completed(_)));

    let highlights = drain_highlights_for_tag(&mut handle.highlight_rx, "security risks");
    assert_eq!(
        highlights.len(),
        1,
        "expected 1 highlight with tag 'security risks', got {}: {highlights:?}",
        highlights.len()
    );
    assert_eq!(highlights[0].tag, "security risks");
    assert_eq!(highlights[0].severity, Severity::Warning);
    assert_eq!(highlights[0].summary, "hardcoded secret found");
}

#[tokio::test]
async fn report_highlight_tag_must_match_focus() {
    let mut focus = FocusContract::empty();
    focus.add("security risks").unwrap();

    let provider = ScriptedProvider::new()
        .then_tool_call(
            "report_highlight",
            json!({
                "tag": "documentation",
                "severity": "info",
                "summary": "missing docs"
            }),
        )
        .then_done()
        .then_text("continuing")
        .then_done();

    let (mut task, mut handle) = make_task(provider, focus, MockFileSystem::new());

    task.run("scan").await;

    let highlights = drain_highlights_for_tag(&mut handle.highlight_rx, "documentation");
    assert!(
        highlights.is_empty(),
        "highlight with invalid tag should not be pushed"
    );
}

// ── Task 9.8: lifecycle transitions during loop ────────────────────────────

#[tokio::test]
async fn lifecycle_transitions_through_normal_path() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, _handle) = Task::new(
        "task-life",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    assert_eq!(task.lifecycle_current(), TaskStatus::Sleeping);

    task.run("read foo.rs").await;

    assert_eq!(task.lifecycle_current(), TaskStatus::Delivered);
}

#[tokio::test]
async fn lifecycle_no_tool_single_turn_stays_pondering_then_delivered() {
    let provider = ScriptedProvider::new().then_text("hello").then_done();
    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    assert_eq!(task.lifecycle_current(), TaskStatus::Sleeping);

    task.run("hi").await;

    assert_eq!(task.lifecycle_current(), TaskStatus::Delivered);
}

#[tokio::test]
async fn lifecycle_heartbeats_emitted_on_transitions() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, mut handle) = Task::new(
        "task-hb",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    task.run("read foo.rs").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    let statuses: Vec<TaskStatus> = heartbeats.iter().map(|hb| hb.status).collect();

    assert!(
        statuses.contains(&TaskStatus::Waking),
        "should have Waking heartbeat: {statuses:?}"
    );
    assert!(
        statuses.contains(&TaskStatus::Pondering),
        "should have Pondering heartbeat: {statuses:?}"
    );
    assert!(
        statuses.contains(&TaskStatus::OnIt),
        "should have OnIt heartbeat: {statuses:?}"
    );
    assert!(
        statuses.contains(&TaskStatus::Delivered),
        "should have Delivered heartbeat: {statuses:?}"
    );
}

// ── Task 9.9: heartbeat pushed each turn ───────────────────────────────────

#[tokio::test]
async fn heartbeat_pushed_each_turn_single() {
    let provider = ScriptedProvider::new().then_text("hello").then_done();
    let (mut task, mut handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("hi").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    assert!(
        !heartbeats.is_empty(),
        "at least one heartbeat should be pushed"
    );
    assert!(
        heartbeats.len() >= 3,
        "single turn should have >= 3 heartbeats (waking + pondering + delivered): got {}",
        heartbeats.len()
    );
}

#[tokio::test]
async fn heartbeat_pushed_each_turn_multi() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;
    let (mut task, mut handle) = Task::new(
        "task-hb-multi",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    task.run("read foo.rs").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    let turns = task.turn_count();
    assert_eq!(turns, 2, "should have run 2 turns");

    // Multi-turn with tools: waking + pondering + on_it + tool_hb + turn_complete_hb + delivered
    assert!(
        heartbeats.len() >= 5,
        "multi-turn should have >= 5 heartbeats: got {}: {heartbeats:?}",
        heartbeats.len()
    );
}

#[tokio::test]
async fn heartbeat_task_id_is_correct() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let (mut task, mut handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("go").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    assert!(!heartbeats.is_empty());
    for hb in &heartbeats {
        assert_eq!(hb.task_id, "task-test");
    }
}

// ── Follow-up queue tests ──────────────────────────────────────────────────

#[tokio::test]
async fn followup_messages_processed_after_turn() {
    let provider = ScriptedProvider::new()
        .then_text("first")
        .then_done()
        .then_text("second")
        .then_done();

    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.push_followup(Message::user("followup question"));

    let outcome = task.run("initial").await;
    assert!(matches!(outcome, TaskOutcome::Completed(_)));

    let has_followup = task
        .active_messages()
        .iter()
        .any(|m| m.content == "followup question");
    assert!(has_followup, "followup message should be in active");
}

#[tokio::test]
async fn empty_followup_queue_returns_zero() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let (task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    assert_eq!(task.followup_len(), 0);
}

// ── Error handling tests ───────────────────────────────────────────────────

#[tokio::test]
async fn exhausted_provider_returns_error() {
    let provider = ScriptedProvider::new();
    let (mut task, _handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    let outcome = task.run("go").await;
    match outcome {
        TaskOutcome::Error(msg) => {
            assert!(
                msg.contains("exhausted") || msg.contains("stream"),
                "error should mention provider failure: {msg}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ── G3.9: Evidence Gate integration test ───────────────────────────────────

/// A workspace backed by a real temp directory so evidence commands
/// can actually execute.
struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Workspace for TempWorkspace {
    fn path(&self) -> &Path {
        &self.path
    }
    fn freeze(&mut self) -> WsResult<()> {
        Ok(())
    }
    fn diff(&self) -> WsResult<ChangeSet> {
        Ok(ChangeSet::default())
    }
    fn merge_into(&self, _target: &Path) -> WsResult<MergeResult> {
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
async fn evidence_gate_failure_prevents_delivered() {
    let dir = tempfile::tempdir().unwrap();

    // Evidence command: succeeds on first run (baseline), fails on
    // second run (verify) by using a marker file.
    let cmd = "sh -c 'if [ -f .gate_marker ]; then echo \"error: gate tripped\" >&2; exit 1; else touch .gate_marker; fi'";

    let provider = ScriptedProvider::new().then_text("done").then_done();

    let tools = ToolRegistry::new();
    let ctx = ToolContext {
        workspace_path: dir.path().to_path_buf(),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
    };
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;

    let (mut task, mut handle) = Task::new(
        "task-evidence",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(TempWorkspace::new(dir.path().to_path_buf())),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );
    task.with_evidence_commands(vec![cmd.to_string()]);

    let outcome = task.run("implement feature X").await;

    assert!(
        !matches!(outcome, TaskOutcome::Completed(_)),
        "Task must NOT complete when evidence gate fails"
    );
    assert_ne!(
        task.lifecycle_current(),
        TaskStatus::Delivered,
        "Task must NOT be Delivered when evidence gate fails"
    );

    // Phase should be back at Two (returned from Three).
    assert_eq!(
        task.current_phase(),
        opca_core::task::Phase::Two,
        "Task should have returned to Phase 2 after evidence failure"
    );

    // An evidence-gate highlight should have been emitted.
    let hls = drain_highlights(&mut handle.highlight_rx);
    let has_gate_failure = hls
        .iter()
        .any(|h| h.tag == "evidence-gate" && h.summary.contains("failed"));
    assert!(
        has_gate_failure,
        "should have an evidence-gate failure highlight"
    );
}

#[tokio::test]
async fn evidence_gate_pass_allows_delivered() {
    let dir = tempfile::tempdir().unwrap();

    // Evidence command that always succeeds.
    let provider = ScriptedProvider::new().then_text("done").then_done();

    let tools = ToolRegistry::new();
    let ctx = ToolContext {
        workspace_path: dir.path().to_path_buf(),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
    };
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;

    let (mut task, _handle) = Task::new(
        "task-evidence-ok",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(TempWorkspace::new(dir.path().to_path_buf())),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );
    task.with_evidence_commands(vec!["true".to_string()]);

    let outcome = task.run("implement feature Y").await;

    assert!(
        matches!(outcome, TaskOutcome::Completed(_)),
        "Task should complete when evidence gate passes"
    );
    assert_eq!(
        task.lifecycle_current(),
        TaskStatus::Delivered,
        "Task should be Delivered when evidence gate passes"
    );
}

// ── G8.7: 3-strike → Stuck integration test ───────────────────────────────

#[tokio::test]
async fn three_strike_evidence_failure_transitions_to_stuck() {
    let dir = tempfile::tempdir().unwrap();

    let cmd = "sh -c 'if [ -f .baseline_marker ]; then echo \"error: type mismatch\" >&2; exit 1; else touch .baseline_marker; fi'";

    let provider = ScriptedProvider::new()
        .then_text("attempt 1")
        .then_done()
        .then_text("attempt 2")
        .then_done()
        .then_text("attempt 3")
        .then_done();

    let tools = ToolRegistry::new();
    let ctx = ToolContext {
        workspace_path: dir.path().to_path_buf(),
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
    };
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;

    let (mut task, mut handle) = Task::new(
        "task-strike",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(TempWorkspace::new(dir.path().to_path_buf())),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );
    task.with_evidence_commands(vec![cmd.to_string()]);

    let outcome = task.run("implement feature Z").await;

    match outcome {
        TaskOutcome::Error(msg) => {
            assert!(
                msg.contains("3 times"),
                "error should mention 3-strike: {msg}"
            );
            assert!(
                msg.contains("type mismatch"),
                "error should mention the repeated issue: {msg}"
            );
        }
        other => panic!("expected Error (Stuck), got {other:?}"),
    }

    assert_eq!(
        task.lifecycle_current(),
        TaskStatus::Stuck,
        "Task should be Stuck after 3 consecutive identical failures"
    );

    let hls = drain_highlights(&mut handle.highlight_rx);
    let gate_failure_count = hls
        .iter()
        .filter(|h| h.tag == "evidence-gate" && h.summary.contains("failed"))
        .count();
    assert_eq!(
        gate_failure_count, 3,
        "should have exactly 3 evidence-gate failure highlights"
    );
}

// ── G6.9: Phase transition integration tests ──────────────────────────────

#[tokio::test]
async fn phase_transitions_through_normal_lifecycle() {
    let fs = MockFileSystem::new();
    fs.insert_file("/workspace/foo.rs", b"data");

    let provider = ScriptedProvider::new()
        .then_tool_call("read", json!({"path": "foo.rs"}))
        .then_done()
        .then_text("done")
        .then_done();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(opca_core::tools::builtin::ReadTool));
    let ctx = make_tool_ctx(fs);
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;

    let (mut task, mut handle) = Task::new(
        "task-phases",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );

    assert_eq!(task.current_phase(), opca_core::task::Phase::Zero);

    task.run("read foo.rs").await;

    assert_eq!(task.lifecycle_current(), TaskStatus::Delivered);

    let highlights = drain_highlights(&mut handle.highlight_rx);
    let transitions: Vec<&Highlight> = highlights
        .iter()
        .filter(|h| h.tag == "phase-transition")
        .collect();

    assert!(
        !transitions.is_empty(),
        "should have phase-transition highlights"
    );

    let summaries: Vec<&str> = transitions.iter().map(|h| h.summary.as_str()).collect();
    assert!(
        summaries
            .iter()
            .any(|s| s.contains("0→1") || s.contains("0→")),
        "should have Phase 0→1 transition: {summaries:?}"
    );
    assert!(
        summaries
            .iter()
            .any(|s| s.contains("→3") || s.contains("Phase 2→3") || s.contains("Phase 1→3")),
        "should have a transition to Phase 3: {summaries:?}"
    );
}

// ── G7.7: TodoWrite integration tests ──────────────────────────────────────

#[tokio::test]
async fn todowrite_tool_updates_heartbeat() {
    let provider = ScriptedProvider::new()
        .then_tool_call(
            "todowrite",
            json!({
                "items": [
                    { "content": "step 1", "status": "completed" },
                    { "content": "step 2", "status": "in_progress" },
                    { "content": "step 3", "status": "pending" }
                ]
            }),
        )
        .then_done()
        .then_text("done")
        .then_done();

    let (mut task, mut handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("do work").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    let tool_hb: Vec<_> = heartbeats.iter().filter(|hb| hb.todo.is_some()).collect();

    assert!(
        !tool_hb.is_empty(),
        "at least one heartbeat should have todo info"
    );

    let todo = tool_hb[0].todo.as_ref().unwrap();
    assert_eq!(todo.total, 3);
    assert_eq!(todo.completed, 1);
    assert_eq!(todo.in_progress.as_deref(), Some("step 2"));
}

#[tokio::test]
async fn trivial_task_heartbeat_has_no_todo() {
    let provider = ScriptedProvider::new().then_text("ok").then_done();
    let (mut task, mut handle) = make_task(provider, FocusContract::empty(), MockFileSystem::new());

    task.run("hi").await;

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    assert!(
        heartbeats.iter().all(|hb| hb.todo.is_none()),
        "trivial task heartbeats should have todo: None"
    );
}
