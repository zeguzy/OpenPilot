use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use opca_core::focus::FocusContract;
use opca_core::lifecycle::TaskStatus;
use opca_core::prompt_system;
use opca_core::provider::{Message, Provider, ProviderStream, ToolDef, anyhow};
use opca_core::task::{Phase, SteeringMessage, Task, TaskOutcome};
use opca_core::tools::{ToolContext, ToolRegistry};
use opca_core::workspace::{ChangeSet, MergeResult, Result as WsResult, Workspace};
use opca_test_utils::{FakeClock, MockFileSystem, MockProcess, ScriptedProvider};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

// ── Test workspace helpers ────────────────────────────────────────────────

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

fn make_ctx(path: PathBuf) -> ToolContext {
    ToolContext {
        workspace_path: path,
        fs: Arc::new(MockFileSystem::new()),
        proc: Arc::new(MockProcess::new()),
        task_id: None,
    }
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

fn drain_highlights(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<opca_core::focus::Highlight>,
) -> Vec<opca_core::focus::Highlight> {
    let mut hls = Vec::new();
    while let Ok(hl) = rx.try_recv() {
        hls.push(hl);
    }
    hls
}

// ── 14.2: Evidence Gate fail-then-succeed → Delivered ─────────────────────

#[tokio::test]
async fn evidence_gate_fail_then_succeed_delivers() {
    let dir = tempfile::tempdir().expect("tempdir");

    let cmd = "sh -c '\
        if [ ! -f .ev_counter ]; then\n\
            echo 1 > .ev_counter\n\
        else\n\
            n=$(cat .ev_counter)\n\
            n=$((n + 1))\n\
            echo $n > .ev_counter\n\
            if [ $n -eq 2 ]; then\n\
                echo \"error: temporary failure\" >&2\n\
                exit 1\n\
            fi\n\
        fi\n\
    '";

    let provider = ScriptedProvider::new()
        .then_text("attempt 1")
        .then_done()
        .then_text("attempt 2 — fixed")
        .then_done();

    let tools = ToolRegistry::new();
    let ctx = make_ctx(dir.path().to_path_buf());
    let clock = Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>;

    let (mut task, mut handle) = Task::new(
        "task-evidence-retry",
        Arc::new(provider) as Arc<dyn Provider>,
        Box::new(TempWorkspace::new(dir.path().to_path_buf())),
        FocusContract::empty(),
        tools,
        ctx,
        clock,
    );
    task.with_evidence_commands(vec![cmd.to_string()]);

    let outcome = task.run("implement feature W").await;

    assert!(
        matches!(outcome, TaskOutcome::Completed(_)),
        "Task should complete after evidence gate retry, got {outcome:?}"
    );
    assert_eq!(
        task.lifecycle_current(),
        TaskStatus::Delivered,
        "Task should be Delivered after evidence gate passes on retry"
    );

    let hls = drain_highlights(&mut handle.highlight_rx);
    let failure_count = hls
        .iter()
        .filter(|h| h.tag == "evidence-gate" && h.summary.contains("failed"))
        .count();
    assert_eq!(
        failure_count, 1,
        "should have exactly 1 evidence-gate failure highlight, got {hls:?}",
    );

    let pass_count = hls
        .iter()
        .filter(|h| h.tag == "evidence-gate" && h.summary.contains("passed"))
        .count();
    assert_eq!(
        pass_count, 1,
        "should have exactly 1 evidence-gate pass highlight, got {hls:?}",
    );
}

// ── 14.4: Clarification flow E2E ──────────────────────────────────────────
//
// An InjectingProvider wraps ScriptedProvider and pre-loads a steering
// message during the first stream() call. The message waits in the channel
// until the second loop iteration's process_steering runs — right after the
// Waiting transition — causing Waiting→OnIt, then the second turn completes.

struct InjectingProvider {
    inner: ScriptedProvider,
    steering_slot: Arc<OnceLock<tokio::sync::mpsc::UnboundedSender<SteeringMessage>>>,
    injected: AtomicBool,
}

impl Provider for InjectingProvider {
    fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send>> {
        if !self.injected.swap(true, Ordering::SeqCst) {
            if let Some(tx) = self.steering_slot.get() {
                tx.send(SteeringMessage::Inject(Message::user(
                    "Use JWT for authentication.",
                )))
                .ok();
            }
        }
        self.inner.stream(messages, tools, system_prompt)
    }
}

#[tokio::test]
async fn clarification_flow_waiting_then_answer_delivers() {
    let steering_slot: Arc<OnceLock<tokio::sync::mpsc::UnboundedSender<SteeringMessage>>> =
        Arc::new(OnceLock::new());

    let injecting = InjectingProvider {
        inner: ScriptedProvider::new()
            .then_tool_call(
                "request_clarification",
                json!({
                    "question": "Should I use JWT or session cookies?",
                    "options": ["JWT", "session cookies"]
                }),
            )
            .then_done()
            .then_text("implemented using JWT")
            .then_done(),
        steering_slot: steering_slot.clone(),
        injected: AtomicBool::new(false),
    };

    let (mut task, mut handle) = Task::new(
        "task-clarify",
        Arc::new(injecting) as Arc<dyn Provider>,
        Box::new(StubWorkspace::new()),
        FocusContract::empty(),
        ToolRegistry::new(),
        make_ctx(PathBuf::from("/workspace")),
        Arc::new(FakeClock::default()) as Arc<dyn opca_core::di::Clock>,
    );

    steering_slot.set(handle.steering_tx.clone()).ok();

    let outcome = task.run("implement authentication").await;

    assert!(
        matches!(outcome, TaskOutcome::Completed(_)),
        "Task should complete after clarification answer, got {outcome:?}"
    );
    assert_eq!(
        task.lifecycle_current(),
        TaskStatus::Delivered,
        "Task should be Delivered after clarification flow completes"
    );

    let heartbeats = drain_heartbeats(&mut handle.heartbeat_rx);
    let statuses: Vec<TaskStatus> = heartbeats.iter().map(|hb| hb.status).collect();
    assert!(
        statuses.contains(&TaskStatus::Waiting),
        "should have a Waiting heartbeat: {statuses:?}"
    );
    assert!(
        statuses.contains(&TaskStatus::Delivered),
        "should have a Delivered heartbeat: {statuses:?}"
    );
}

// ── 14.13: Prompt template snapshot tests ────────────────────────────────

#[test]
fn snapshot_orchestrator_prompt() {
    let prompt = prompt_system::orchestrator::orchestrator_prompt();
    insta::assert_snapshot!(prompt);
}

#[test]
fn snapshot_orchestrator_prompt_version() {
    let v = prompt_system::orchestrator::PROMPT_VERSION;
    assert!(
        v.starts_with("orchestrator-v"),
        "version must be tagged: {v}"
    );
    insta::assert_snapshot!("orchestrator_prompt_version", v);
}

#[test]
fn snapshot_audit_prompt_with_dimensions() {
    let dims = vec![
        "compilation".to_string(),
        "tests".to_string(),
        "security".to_string(),
    ];
    let prompt = prompt_system::audit::audit_prompt(&dims);
    insta::assert_snapshot!(prompt);
}

#[test]
fn snapshot_audit_prompt_no_dimensions() {
    let prompt = prompt_system::audit::audit_prompt(&[]);
    insta::assert_snapshot!(prompt);
}

#[test]
fn snapshot_audit_prompt_version() {
    let v = prompt_system::audit::PROMPT_VERSION;
    assert!(v.starts_with("audit-v"), "version must be tagged: {v}");
    insta::assert_snapshot!("audit_prompt_version", v);
}

#[test]
fn snapshot_task_prompt_version() {
    let v = prompt_system::task::PROMPT_VERSION;
    assert!(v.starts_with("task-v"), "version must be tagged: {v}");
    insta::assert_snapshot!("task_prompt_version", v);
}

#[test]
fn snapshot_task_prompt_phase_zero() {
    let prompt = prompt_system::task::build_task_prompt(Phase::Zero);
    insta::assert_snapshot!(prompt);
}

#[test]
fn snapshot_task_prompt_phase_three() {
    let prompt = prompt_system::task::build_task_prompt(Phase::Three);
    insta::assert_snapshot!(prompt);
}

#[test]
fn snapshot_continuation_seed_version() {
    let v = prompt_system::continuation::retrospective::PROMPT_VERSION;
    assert!(
        v.starts_with("continuation-v"),
        "version must be tagged: {v}"
    );
    insta::assert_snapshot!("continuation_seed_version", v);
}

#[test]
fn snapshot_continuation_seed_with_budget_and_history() {
    use std::time::Duration;

    use opca_core::audit::{AuditVerdict, Finding};
    use opca_core::continuation::budget::ContinuationBudget;
    use opca_core::continuation::chain::IterationRecord;
    use opca_core::focus::Severity;
    use opca_core::prompt_system::continuation::retrospective::continuation_seed;

    let mut budget = ContinuationBudget::new(10, 5.0, Duration::from_secs(1800), 2);
    budget.record_iteration(0.05);
    budget.record_iteration(0.08);

    let history = vec![
        IterationRecord {
            task_id: "task-1".to_string(),
            iteration: 1,
            verdict: Some(AuditVerdict::NeedsFix),
            cost_usd: 0.05,
            duration: Duration::from_secs(10),
            diff_summary: "attempted fix in auth.rs".to_string(),
        },
        IterationRecord {
            task_id: "task-2".to_string(),
            iteration: 2,
            verdict: Some(AuditVerdict::NeedsFix),
            cost_usd: 0.08,
            duration: Duration::from_secs(20),
            diff_summary: "restructured login flow".to_string(),
        },
    ];

    let findings = vec![
        Finding {
            severity: Severity::Warning,
            location: "src/auth.rs".to_string(),
            issue: "test_login still fails".to_string(),
        },
        Finding {
            severity: Severity::Info,
            location: "src/session.rs".to_string(),
            issue: "session timeout not handled".to_string(),
        },
    ];

    let seed = continuation_seed(3, &findings, &budget, &history);
    insta::assert_snapshot!(seed);
}

#[test]
fn snapshot_audit_report_json_with_justification() {
    use opca_core::audit::{AuditDecision, AuditReport, AuditVerdict, Finding};
    use opca_core::focus::Severity;

    let report = AuditReport {
        task_id: "task-audit-1".to_string(),
        verdict: AuditVerdict::NeedsFix,
        confidence: 0.85,
        findings: vec![Finding {
            severity: Severity::Warning,
            location: "src/lib.rs:42".to_string(),
            issue: "Unhandled Result variant in match expression".to_string(),
        }],
        summary: "Found one major issue requiring a fix.".to_string(),
        justification: "Decision tree rule 4: major finding in src/lib.rs:42 \
            mandates NeedsFix verdict."
            .to_string(),
    };
    let decision = AuditDecision::accept(report);
    let json = serde_json::to_string_pretty(&decision).expect("serialize");
    insta::assert_snapshot!(json);
}

// ── Prompt content verification ───────────────────────────────────────────

#[test]
fn orchestrator_prompt_contains_dispatch_tool_not_prefix() {
    let prompt = prompt_system::orchestrator::orchestrator_prompt();
    assert!(
        prompt.contains("dispatch_task"),
        "orchestrator prompt must reference the dispatch_task tool"
    );
    assert!(
        !prompt.contains("OPCA_DISPATCH"),
        "orchestrator prompt must NOT reference the legacy OPCA_DISPATCH prefix"
    );
}

#[test]
fn task_prompt_phase_two_contains_hard_blocks() {
    let prompt = prompt_system::task::build_task_prompt(Phase::Two);
    assert!(prompt.contains("Hard Blocks"));
    assert!(prompt.contains("unsafe"));
    assert!(prompt.contains(".unwrap()"));
}

#[test]
fn task_prompt_phase_three_contains_evidence_gate() {
    let prompt = prompt_system::task::build_task_prompt(Phase::Three);
    assert!(prompt.contains("Evidence Gate"));
}

#[test]
fn audit_prompt_contains_decision_tree_and_justification() {
    let dims = vec!["security".to_string()];
    let prompt = prompt_system::audit::audit_prompt(&dims);
    assert!(prompt.contains("Decision tree"));
    assert!(prompt.contains("justification"));
    assert!(
        prompt.contains("critical") && prompt.contains("major"),
        "audit prompt must define severity levels"
    );
}
