use opca_core::audit::{AuditDecision, AuditReport, AuditVerdict, Finding};
use opca_core::focus::Severity;

// ── Task 11.11: Orchestrator overrides fail → accept ──────────────────────

fn fail_report(task_id: &str) -> AuditReport {
    AuditReport {
        task_id: task_id.to_string(),
        verdict: AuditVerdict::Fail,
        confidence: 0.3,
        findings: vec![Finding {
            severity: Severity::Blocking,
            location: "tests/auth_test.rs:45".to_string(),
            issue: "test_auth_token_refresh failed".to_string(),
        }],
        summary: "test_auth_token_refresh is failing".to_string(),
    }
}

#[test]
fn orchestrator_overrides_fail_to_pass_with_reason() {
    let report = fail_report("task-override-001");
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Pass,
        "test_auth_token_refresh was already broken before this task, confirmed via git blame",
    );

    assert!(decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::Pass);
    assert_eq!(
        decision.override_verdict,
        Some(AuditVerdict::Pass),
        "override verdict should be stored"
    );
    assert_eq!(
        decision.report.verdict,
        AuditVerdict::Fail,
        "original report verdict should be preserved"
    );
    assert!(
        decision
            .override_reason
            .as_ref()
            .is_some_and(|r| r.contains("already broken"))
    );
}

#[test]
fn accept_decision_does_not_override() {
    let report = fail_report("task-accept-002");
    let decision = AuditDecision::accept(report);

    assert!(!decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::Fail);
    assert!(decision.override_verdict.is_none());
    assert!(decision.override_reason.is_none());
}

#[test]
fn override_to_warn_keeps_original_report() {
    let report = fail_report("task-override-warn");
    let original_confidence = report.confidence;
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Warn,
        "issue is minor, proceed with caution",
    );

    assert_eq!(decision.effective_verdict(), AuditVerdict::Warn);
    assert!(
        (decision.report.confidence - original_confidence).abs() < f64::EPSILON,
        "report data should be unchanged"
    );
    assert_eq!(decision.report.verdict, AuditVerdict::Fail);
}

#[test]
fn override_to_fail_from_pass() {
    let report = AuditReport {
        task_id: "task-escalate".to_string(),
        verdict: AuditVerdict::Pass,
        confidence: 0.9,
        findings: vec![],
        summary: "audit said pass".to_string(),
    };
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Fail,
        "orchestrator caught issue audit missed",
    );

    assert!(decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::Fail);
}

#[test]
fn decision_serializes_with_override_fields() {
    let report = fail_report("task-serde");
    let decision = AuditDecision::override_to(report, AuditVerdict::Pass, "pre-existing");

    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("\"override_verdict\""));
    assert!(json.contains("\"pass\""));
    assert!(json.contains("\"override_reason\""));
    assert!(json.contains("pre-existing"));
}

#[test]
fn decision_accept_serializes_without_override_fields() {
    let report = fail_report("task-serde-accept");
    let decision = AuditDecision::accept(report);

    let json = serde_json::to_string(&decision).unwrap();
    assert!(
        !json.contains("override_verdict"),
        "accept decision should skip override fields: {json}"
    );
    assert!(
        !json.contains("override_reason"),
        "accept decision should skip override fields: {json}"
    );
}

#[test]
fn full_lifecycle_audit_then_override() {
    let report = fail_report("task-lifecycle");

    assert_eq!(report.verdict, AuditVerdict::Fail);

    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Pass,
        "recall: these tests were broken in commit abc123",
    );

    assert_eq!(decision.effective_verdict(), AuditVerdict::Pass);
    assert!(decision.was_overridden());

    let json = serde_json::to_string(&decision).unwrap();
    let back: AuditDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(decision, back, "decision should round-trip");
}
