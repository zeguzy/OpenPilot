use insta::assert_json_snapshot;

use opca_core::audit::{AuditDecision, AuditReport, AuditVerdict, Finding};
use opca_core::focus::Severity;

// ── Task 11.9: Snapshot tests for Audit report format ─────────────────────

#[test]
fn snapshot_pass_report() {
    let report = AuditReport {
        task_id: "task-pass-001".to_string(),
        verdict: AuditVerdict::Pass,
        confidence: 0.95,
        findings: vec![],
        summary: "All checks passed, diff is clean".to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_warn_report() {
    let report = AuditReport {
        task_id: "task-warn-002".to_string(),
        verdict: AuditVerdict::Warn,
        confidence: 0.7,
        findings: vec![Finding {
            severity: Severity::Warning,
            location: "src/auth.rs:42".to_string(),
            issue: "Missing null check on user input".to_string(),
        }],
        summary: "Minor issue found in auth module".to_string(),
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_fail_report() {
    let report = AuditReport {
        task_id: "task-fail-003".to_string(),
        verdict: AuditVerdict::Fail,
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
    };
    assert_json_snapshot!(report);
}

#[test]
fn snapshot_decision_accept() {
    let report = AuditReport {
        task_id: "task-decision-accept".to_string(),
        verdict: AuditVerdict::Pass,
        confidence: 0.9,
        findings: vec![],
        summary: "Clean".to_string(),
    };
    let decision = AuditDecision::accept(report);
    assert_json_snapshot!(decision);
}

#[test]
fn snapshot_decision_override() {
    let report = AuditReport {
        task_id: "task-decision-override".to_string(),
        verdict: AuditVerdict::Fail,
        confidence: 0.3,
        findings: vec![Finding {
            severity: Severity::Blocking,
            location: "tests/integration.rs:100".to_string(),
            issue: "3 tests failed".to_string(),
        }],
        summary: "Tests are failing".to_string(),
    };
    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::Pass,
        "tests were pre-existing failures, not caused by this task",
    );
    assert_json_snapshot!(decision);
}

#[test]
fn report_json_serializes_verdict_lowercase() {
    let report = AuditReport {
        task_id: "t".to_string(),
        verdict: AuditVerdict::Warn,
        confidence: 0.5,
        findings: vec![],
        summary: "test".to_string(),
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        json.contains("\"warn\""),
        "verdict should serialize as lowercase: {json}"
    );
    assert!(!json.contains("\"Warn\""), "no uppercase: {json}");
}

#[test]
fn report_json_roundtrip() {
    let report = AuditReport {
        task_id: "roundtrip".to_string(),
        verdict: AuditVerdict::Fail,
        confidence: 0.15,
        findings: vec![Finding {
            severity: Severity::Info,
            location: "x.rs:1".to_string(),
            issue: "note".to_string(),
        }],
        summary: "roundtrip test".to_string(),
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: AuditReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}
