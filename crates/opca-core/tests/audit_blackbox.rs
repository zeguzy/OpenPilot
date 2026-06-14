use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use opca_core::audit::build_audit_focus;
use opca_core::audit::{AuditAgent, AuditVerdict, ModelTier};
use opca_core::focus::{FocusContract, Severity};
use opca_core::provider::{Message, Provider};
use opca_core::workspace::ChangeSet;
use opca_test_utils::ScriptedProvider;

fn make_agent(
    provider: ScriptedProvider,
    diff: ChangeSet,
    task_memory: Vec<Message>,
) -> AuditAgent {
    let task_focus = FocusContract::new(vec!["security".to_string()]);
    let focus = build_audit_focus(&task_focus, &[]);
    AuditAgent::new(
        Arc::new(provider) as Arc<dyn Provider>,
        "task-audit-test",
        PathBuf::from("/workspace"),
        diff,
        Arc::new(Mutex::new(task_memory)),
        focus,
        ModelTier::Cheap,
    )
}

fn clean_diff() -> ChangeSet {
    ChangeSet {
        added: vec![PathBuf::from("src/feature.rs")],
        modified: vec![PathBuf::from("src/lib.rs")],
        deleted: vec![],
    }
}

// ── Task 11.6: black-box pass/warn/fail verdicts ──────────────────────────

#[tokio::test]
async fn black_box_pass_verdict() {
    let response = r#"{"verdict":"pass","confidence":0.95,"findings":[],"summary":"all good"}"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let report = agent.audit().await.unwrap();

    assert_eq!(report.task_id, "task-audit-test");
    assert_eq!(report.verdict, AuditVerdict::Pass);
    assert!((report.confidence - 0.95).abs() < 1e-9);
    assert!(report.findings.is_empty());
    assert_eq!(report.summary, "all good");
}

#[tokio::test]
async fn black_box_warn_verdict_with_findings() {
    let response = r#"{
        "verdict": "warn",
        "confidence": 0.7,
        "findings": [
            {"severity": "warning", "location": "src/auth.rs:42", "issue": "missing null check"}
        ],
        "summary": "minor issue found in auth"
    }"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let report = agent.audit().await.unwrap();

    assert_eq!(report.verdict, AuditVerdict::Warn);
    assert!((report.confidence - 0.7).abs() < 1e-9);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].severity, Severity::Warning);
    assert_eq!(report.findings[0].location, "src/auth.rs:42");
    assert_eq!(report.findings[0].issue, "missing null check");
    assert_eq!(report.summary, "minor issue found in auth");
}

#[tokio::test]
async fn black_box_fail_verdict_with_blocking_finding() {
    let response = r#"{
        "verdict": "fail",
        "confidence": 0.3,
        "findings": [
            {"severity": "blocking", "location": "src/crypto.rs:10", "issue": "hardcoded secret key"}
        ],
        "summary": "critical security issue detected"
    }"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let report = agent.audit().await.unwrap();

    assert_eq!(report.verdict, AuditVerdict::Fail);
    assert!((report.confidence - 0.3).abs() < 1e-9);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].severity, Severity::Blocking);
    assert_eq!(report.findings[0].location, "src/crypto.rs:10");
    assert_eq!(report.summary, "critical security issue detected");
}

#[tokio::test]
async fn black_box_task_id_always_set_from_agent() {
    let response = r#"{"verdict":"pass","confidence":1.0,"findings":[],"summary":"ok"}"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let report = agent.audit().await.unwrap();
    assert_eq!(
        report.task_id, "task-audit-test",
        "task_id must come from the agent, not the provider response"
    );
}

#[tokio::test]
async fn black_box_multiple_findings() {
    let response = r#"{
        "verdict": "warn",
        "confidence": 0.6,
        "findings": [
            {"severity": "warning", "location": "a.rs:1", "issue": "issue 1"},
            {"severity": "info", "location": "b.rs:2", "issue": "issue 2"},
            {"severity": "blocking", "location": "c.rs:3", "issue": "issue 3"}
        ],
        "summary": "multiple issues"
    }"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let report = agent.audit().await.unwrap();

    assert_eq!(report.findings.len(), 3);
    assert_eq!(report.findings[0].severity, Severity::Warning);
    assert_eq!(report.findings[1].severity, Severity::Info);
    assert_eq!(report.findings[2].severity, Severity::Blocking);
}

#[tokio::test]
async fn black_box_invalid_json_returns_error() {
    let provider = ScriptedProvider::new().then_text("not json").then_done();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let result = agent.audit().await;
    assert!(result.is_err(), "invalid JSON should produce an error");
}

#[tokio::test]
async fn black_box_provider_exhausted_returns_error() {
    let provider = ScriptedProvider::new();
    let agent = make_agent(provider, clean_diff(), vec![]);

    let result = agent.audit().await;
    assert!(result.is_err());
}
