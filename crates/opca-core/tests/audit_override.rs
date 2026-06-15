use opca_core::audit::{AuditDecision, AuditReport, AuditVerdict, Finding};
use opca_core::focus::Severity;

// ── Task 11.11: Orchestrator overrides fail → accept ──────────────────────

fn fail_report(task_id: &str) -> AuditReport {
    AuditReport {
        task_id: task_id.to_string(),
        verdict: AuditVerdict::NeedsFix,
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
        AuditVerdict::Confirmed,
        "test_auth_token_refresh was already broken before this task, confirmed via git blame",
    );

    assert!(decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::Confirmed);
    assert_eq!(
        decision.override_verdict,
        Some(AuditVerdict::Confirmed),
        "override verdict should be stored"
    );
    assert_eq!(
        decision.report.verdict,
        AuditVerdict::NeedsFix,
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
    assert_eq!(decision.effective_verdict(), AuditVerdict::NeedsFix);
    assert!(decision.override_verdict.is_none());
    assert!(decision.override_reason.is_none());
}

#[test]
fn override_to_false_positive_keeps_original_report() {
    let report = fail_report("task-override-false-positive");
    let original_confidence = report.confidence;
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::FalsePositive,
        "issue is minor, proceed with caution",
    );

    assert_eq!(decision.effective_verdict(), AuditVerdict::FalsePositive);
    assert!(
        (decision.report.confidence - original_confidence).abs() < f64::EPSILON,
        "report data should be unchanged"
    );
    assert_eq!(decision.report.verdict, AuditVerdict::NeedsFix);
}

#[test]
fn override_to_needs_fix_from_confirmed() {
    let report = AuditReport {
        task_id: "task-escalate".to_string(),
        verdict: AuditVerdict::Confirmed,
        confidence: 0.9,
        findings: vec![],
        summary: "audit said confirmed".to_string(),
    };
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::NeedsFix,
        "orchestrator caught issue audit missed",
    );

    assert!(decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::NeedsFix);
}

#[test]
fn decision_serializes_with_override_fields() {
    let report = fail_report("task-serde");
    let decision = AuditDecision::override_to(report, AuditVerdict::Confirmed, "pre-existing");

    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("\"override_verdict\""));
    assert!(json.contains("\"confirmed\""));
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

    assert_eq!(report.verdict, AuditVerdict::NeedsFix);

    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Confirmed,
        "recall: these tests were broken in commit abc123",
    );

    assert_eq!(decision.effective_verdict(), AuditVerdict::Confirmed);
    assert!(decision.was_overridden());

    let json = serde_json::to_string(&decision).unwrap();
    let back: AuditDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(decision, back, "decision should round-trip");
}
