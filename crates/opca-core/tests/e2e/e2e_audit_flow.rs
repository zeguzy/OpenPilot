//! Task 17.4 — E2E: Task triggers Audit → Audit verdict → Orchestrator decision.
//!
//! Creates an `AuditAgent` backed by a `ScriptedProvider` that returns a JSON
//! audit report. Verifies the report is parsed correctly, an `AuditDecision`
//! is derived from it, and an Orchestrator override can flip the verdict.

use std::sync::{Arc, Mutex};

use opca_core::audit::{AuditAgent, AuditDecision, AuditVerdict, ModelTier};
use opca_core::workspace::ChangeSet;
use opca_test_utils::ScriptedProvider;

fn pass_report_json() -> String {
    serde_json::json!({
        "verdict": "confirmed",
        "confidence": 0.95,
        "findings": [],
        "summary": "all checks passed"
    })
    .to_string()
}

fn fail_report_json() -> String {
    serde_json::json!({
        "verdict": "needs_fix",
        "confidence": 0.85,
        "findings": [
            {
                "severity": "blocking",
                "location": "src/auth.rs:42",
                "issue": "removed authentication check"
            }
        ],
        "summary": "critical security regression"
    })
    .to_string()
}

#[tokio::test]
#[ignore = "E2E: audit flow with scripted LLM"]
async fn e2e_audit_pass_verdict_accepted() {
    let provider = Arc::new(
        ScriptedProvider::new()
            .then_text(&pass_report_json())
            .then_done(),
    ) as Arc<dyn opca_core::provider::Provider>;

    let diff = ChangeSet {
        added: vec![std::path::PathBuf::from("docs/guide.md")],
        modified: vec![],
        deleted: vec![],
    };

    let agent = AuditAgent::new(
        provider,
        "e2e-audit-pass",
        std::path::PathBuf::from("/workspace"),
        diff,
        Arc::new(Mutex::new(vec![])),
        vec!["security risks".to_string()],
        ModelTier::Cheap,
    );

    let report = agent.audit().await.expect("audit");

    assert_eq!(report.verdict, AuditVerdict::Confirmed);
    assert!(report.confidence > 0.9);
    assert!(report.findings.is_empty());

    let decision = AuditDecision::accept(report);
    assert!(!decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::Confirmed);
}

#[tokio::test]
#[ignore = "E2E: audit flow with scripted LLM"]
async fn e2e_audit_fail_verdict_overridden() {
    let provider = Arc::new(
        ScriptedProvider::new()
            .then_text(&fail_report_json())
            .then_done(),
    ) as Arc<dyn opca_core::provider::Provider>;

    let diff = ChangeSet {
        added: vec![],
        modified: vec![],
        deleted: vec![std::path::PathBuf::from("src/auth.rs")],
    };

    let agent = AuditAgent::new(
        provider,
        "e2e-audit-fail",
        std::path::PathBuf::from("/workspace"),
        diff,
        Arc::new(Mutex::new(vec![])),
        vec!["security risks".to_string()],
        ModelTier::Strong,
    );

    let report = agent.audit().await.expect("audit");

    assert_eq!(report.verdict, AuditVerdict::NeedsFix);
    assert_eq!(report.findings.len(), 1);

    let decision = AuditDecision::override_to(
        report,
        AuditVerdict::FalsePositive,
        "false positive: auth check moved to middleware",
    );

    assert!(decision.was_overridden());
    assert_eq!(decision.effective_verdict(), AuditVerdict::FalsePositive);
    assert_eq!(
        decision.override_reason.as_deref(),
        Some("false positive: auth check moved to middleware")
    );
}
