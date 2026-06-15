use insta::assert_json_snapshot;

use opca_core::audit::{AuditDecision, AuditReport, AuditVerdict, Finding};
use opca_core::focus::Severity;

// ── Task 11.9: Snapshot tests for Audit report format ─────────────────────

#[test]
fn snapshot_confirmed_report() {
    let report = AuditReport {
        task_id: "task-confirmed-001".to_string(),
        verdict: AuditVerdict::Confirmed,
        confidence: 0.95,
        findings: vec![],
        summary: "All checks passed, diff is clean".to_string(),
        justification: "Decision tree rule 4: only info/minor findings, confidence ≥ 0.5."
            .to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_false_positive_report() {
    let report = AuditReport {
        task_id: "task-false-positive-002".to_string(),
        verdict: AuditVerdict::FalsePositive,
        confidence: 0.7,
        findings: vec![Finding {
            severity: Severity::Warning,
            location: "src/auth.rs:42".to_string(),
            issue: "Missing null check on user input".to_string(),
        }],
        summary: "Minor issue found in auth module".to_string(),
        justification: "Diff does not contain the claimed auth refactor.".to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_needs_fix_report() {
    let report = AuditReport {
        task_id: "task-needs-fix-003".to_string(),
        verdict: AuditVerdict::NeedsFix,
        confidence: 0.2,
        findings: vec![
            Finding {
                severity: Severity::Blocking,
                location: "src/crypto.rs:10".to_string(),
                issue: "Hardcoded secret key in source".to_string(),
            },
            Finding {
                severity: Severity::Warning,
                location: "src/crypto.rs:25".to_string(),
                issue: "Weak hash algorithm (MD5)".to_string(),
            },
        ],
        summary: "Critical security vulnerabilities detected".to_string(),
        justification: "Decision tree rule 2: major finding (hardcoded secret) mandates NeedsFix."
            .to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_needs_human_review_report() {
    let report = AuditReport {
        task_id: "task-needs-human-review-004".to_string(),
        verdict: AuditVerdict::NeedsHumanReview,
        confidence: 0.4,
        findings: vec![Finding {
            severity: Severity::Info,
            location: "src/legacy.rs:100".to_string(),
            issue: "Ambiguous refactor, cannot verify correctness".to_string(),
        }],
        summary: "Cannot determine correctness automatically".to_string(),
        justification: "Refactor is too ambiguous to verify automatically; escalating.".to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_decision_accept() {
    let report = AuditReport {
        task_id: "task-decision-accept".to_string(),
        verdict: AuditVerdict::Confirmed,
        confidence: 0.9,
        findings: vec![],
        summary: "Clean".to_string(),
        justification: String::new(),
    };
    let decision = AuditDecision::accept(report);
    assert_json_snapshot!(decision);
}

#[test]
fn snapshot_decision_override() {
    let report = AuditReport {
        task_id: "task-decision-override".to_string(),
        verdict: AuditVerdict::NeedsFix,
        confidence: 0.3,
        findings: vec![Finding {
            severity: Severity::Blocking,
            location: "tests/integration.rs:100".to_string(),
            issue: "3 tests failed".to_string(),
        }],
        summary: "Tests are failing".to_string(),
        justification: String::new(),
    };
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Confirmed,
        "tests were pre-existing failures, not caused by this task",
    );
    assert_json_snapshot!(decision);
}

#[test]
fn report_json_serializes_verdict_snake_case() {
    let report = AuditReport {
        task_id: "t".to_string(),
        verdict: AuditVerdict::FalsePositive,
        confidence: 0.5,
        findings: vec![],
        summary: "test".to_string(),
        justification: String::new(),
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        json.contains("\"false_positive\""),
        "verdict should serialize as snake_case: {json}"
    );
    assert!(!json.contains("\"FalsePositive\""), "no PascalCase: {json}");
}

#[test]
fn report_json_roundtrip() {
    let report = AuditReport {
        task_id: "roundtrip".to_string(),
        verdict: AuditVerdict::NeedsFix,
        confidence: 0.15,
        findings: vec![Finding {
            severity: Severity::Info,
            location: "x.rs:1".to_string(),
            issue: "note".to_string(),
        }],
        summary: "roundtrip test".to_string(),
        justification: "test justification".to_string(),
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: AuditReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}
