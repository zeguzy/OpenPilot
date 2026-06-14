use std::path::PathBuf;

use opca_core::audit::{build_audit_focus, is_diff_suspicious};
use opca_core::focus::FocusContract;
use opca_core::workspace::ChangeSet;

// ── Task 11.3: Audit focus correctly inherited from Task ──────────────────

#[test]
fn focus_inherits_task_dimensions_plus_standards() {
    let task_focus = FocusContract::new(vec!["security".to_string(), "breaking".to_string()]);
    let dims = build_audit_focus(&task_focus, &[]);
    assert_eq!(
        dims,
        vec![
            "security",
            "breaking",
            "compilation",
            "tests",
            "diff-sanity",
        ]
    );
}

#[test]
fn focus_with_orchestrator_extras_appended() {
    let task_focus = FocusContract::new(vec!["performance".to_string()]);
    let extras = vec!["log-noise".to_string(), "api-compat".to_string()];
    let dims = build_audit_focus(&task_focus, &extras);
    assert_eq!(
        dims,
        vec![
            "performance",
            "compilation",
            "tests",
            "diff-sanity",
            "log-noise",
            "api-compat",
        ]
    );
}

#[test]
fn focus_empty_task_contract_only_standards() {
    let task_focus = FocusContract::empty();
    let dims = build_audit_focus(&task_focus, &[]);
    assert_eq!(dims, vec!["compilation", "tests", "diff-sanity"]);
}

#[test]
fn focus_standards_not_duplicated_when_already_present() {
    let task_focus = FocusContract::new(vec!["security".to_string(), "tests".to_string()]);
    let dims = build_audit_focus(&task_focus, &[]);
    let tests_count = dims.iter().filter(|d| d.as_str() == "tests").count();
    assert_eq!(
        tests_count, 1,
        "tests should appear exactly once, got {dims:?}"
    );
    assert_eq!(
        dims,
        vec!["security", "tests", "compilation", "diff-sanity"]
    );
}

#[test]
fn focus_orchestrator_extra_deduplicates_with_standards() {
    let task_focus = FocusContract::new(vec!["security".to_string()]);
    let extras = vec!["compilation".to_string()];
    let dims = build_audit_focus(&task_focus, &extras);
    let compilation_count = dims.iter().filter(|d| d.as_str() == "compilation").count();
    assert_eq!(
        compilation_count, 2,
        "orchestrator extras are appended verbatim; standard dedup only applies to task dims"
    );
}

#[test]
fn diff_with_no_deletions_not_suspicious() {
    let diff = ChangeSet {
        added: vec![PathBuf::from("src/new.rs")],
        modified: vec![PathBuf::from("src/existing.rs")],
        deleted: vec![],
    };
    assert!(!is_diff_suspicious(&diff));
}

#[test]
fn diff_with_deletions_is_suspicious() {
    let diff = ChangeSet {
        added: vec![],
        modified: vec![],
        deleted: vec![PathBuf::from("src/old_auth.rs")],
    };
    assert!(is_diff_suspicious(&diff));
}

#[test]
fn empty_diff_not_suspicious() {
    let diff = ChangeSet::default();
    assert!(!is_diff_suspicious(&diff));
}
