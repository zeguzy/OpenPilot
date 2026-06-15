use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use opca_core::audit::build_audit_focus;
use opca_core::audit::{AuditAgent, ModelTier, is_diff_suspicious};
use opca_core::focus::FocusContract;
use opca_core::provider::{Message, Provider};
use opca_core::workspace::ChangeSet;
use opca_test_utils::ScriptedProvider;

fn suspicious_diff() -> ChangeSet {
    ChangeSet {
        added: vec![PathBuf::from("src/new_feature.rs")],
        modified: vec![PathBuf::from("src/lib.rs")],
        deleted: vec![PathBuf::from("src/old_auth.rs")],
    }
}

fn clean_diff() -> ChangeSet {
    ChangeSet {
        added: vec![PathBuf::from("src/feature.rs")],
        modified: vec![PathBuf::from("src/lib.rs")],
        deleted: vec![],
    }
}

fn make_agent(
    provider: ScriptedProvider,
    diff: ChangeSet,
    task_memory: Vec<Message>,
) -> AuditAgent {
    let task_focus = FocusContract::new(vec!["security".to_string()]);
    let focus = build_audit_focus(&task_focus, &[]);
    AuditAgent::new(
        Arc::new(provider) as Arc<dyn Provider>,
        "task-suspicious",
        PathBuf::from("/workspace"),
        diff,
        Arc::new(Mutex::new(task_memory)),
        focus,
        ModelTier::Cheap,
    )
}

// ── Task 11.7: deep dive triggered when diff is suspicious ────────────────

#[test]
fn suspicious_diff_detected_by_heuristic() {
    assert!(is_diff_suspicious(&suspicious_diff()));
    assert!(!is_diff_suspicious(&clean_diff()));
}

#[tokio::test]
async fn deep_dive_reads_task_reasoning_for_suspicious_diff() {
    let task_memory = vec![
        Message::assistant(
            "I decided to delete old_auth.rs because the new auth module replaces it",
        ),
        Message::user("make sure the migration is complete"),
        Message::assistant("I deleted old_auth.rs to clean up dead code"),
    ];

    let response = r#"{"verdict":"false_positive","confidence":0.5,"findings":[],"summary":"suspicious deletion"}"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, suspicious_diff(), task_memory);

    let report = agent.audit().await.unwrap();
    assert_eq!(
        report.verdict,
        opca_core::audit::AuditVerdict::FalsePositive
    );

    let context = agent
        .deep_dive_task_context("deleted old_auth")
        .await
        .unwrap();
    assert!(
        !context.is_empty(),
        "deep dive should find messages about the deletion"
    );
    assert!(
        context.iter().any(|m| m.content.contains("old_auth.rs")),
        "deep dive should find the reasoning about old_auth.rs"
    );
}

#[tokio::test]
async fn deep_dive_returns_empty_when_keyword_not_found() {
    let task_memory = vec![
        Message::assistant("working on the UI components"),
        Message::user("update the button styles"),
    ];

    let provider = ScriptedProvider::new()
        .then_text(r#"{"verdict":"confirmed","confidence":0.9,"findings":[],"summary":"ok"}"#)
        .then_done();
    let agent = make_agent(provider, suspicious_diff(), task_memory);

    let context = agent
        .deep_dive_task_context("deleted function crypto")
        .await
        .unwrap();
    assert!(
        context.is_empty(),
        "deep dive should return empty when no messages match"
    );
}

#[tokio::test]
async fn deep_dive_empty_query_returns_all_messages() {
    let task_memory = vec![
        Message::assistant("first message"),
        Message::assistant("second message"),
        Message::assistant("third message"),
    ];

    let provider = ScriptedProvider::new()
        .then_text(r#"{"verdict":"confirmed","confidence":0.9,"findings":[],"summary":"ok"}"#)
        .then_done();
    let agent = make_agent(provider, clean_diff(), task_memory);

    let context = agent.deep_dive_task_context("").await.unwrap();
    assert_eq!(context.len(), 3, "empty query should return all messages");
}

#[tokio::test]
async fn deep_dive_filters_by_keyword_case_insensitive() {
    let task_memory = vec![
        Message::assistant("The Auth module was refactored"),
        Message::assistant("Updated the database connection pool"),
    ];

    let provider = ScriptedProvider::new()
        .then_text(r#"{"verdict":"confirmed","confidence":0.9,"findings":[],"summary":"ok"}"#)
        .then_done();
    let agent = make_agent(provider, suspicious_diff(), task_memory);

    let context = agent.deep_dive_task_context("auth").await.unwrap();
    assert_eq!(context.len(), 1, "should match case-insensitively");
    assert!(context[0].content.contains("Auth"));
}

#[tokio::test]
async fn audit_with_deep_dive_escalates_on_flawed_reasoning() {
    let task_memory = vec![
        Message::assistant("I think deleting old_auth.rs is fine"),
        Message::assistant("not sure if anything depends on it"),
    ];

    let response = r#"{"verdict":"false_positive","confidence":0.5,"findings":[],"summary":"suspicious deletion"}"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, suspicious_diff(), task_memory);

    let report = agent.audit_with_deep_dive().await.unwrap();
    assert_eq!(
        report.verdict,
        opca_core::audit::AuditVerdict::NeedsFix,
        "flawed reasoning should escalate to needs_fix"
    );
    assert!(
        !report.findings.is_empty(),
        "escalated report should include a deep-dive finding"
    );
}

#[tokio::test]
async fn audit_with_deep_dive_skipped_for_clean_diff() {
    let task_memory = vec![Message::assistant("I think this is wrong")];

    let response = r#"{"verdict":"false_positive","confidence":0.5,"findings":[],"summary":"ok"}"#;
    let provider = ScriptedProvider::new().then_text(response).then_done();
    let agent = make_agent(provider, clean_diff(), task_memory);

    let report = agent.audit_with_deep_dive().await.unwrap();
    assert_eq!(
        report.verdict,
        opca_core::audit::AuditVerdict::FalsePositive,
        "clean diff should not trigger deep dive escalation"
    );
    assert!(report.findings.is_empty());
}
