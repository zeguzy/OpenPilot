//! Property-based tests for the prompt-system refactor (G14).
//!
//! Covers:
//! - **14.11**: Evidence Gate baseline diff is sound — identical workspace
//!   states never produce false positives.
//! - **14.12**: Issue signature normalization is deterministic and survives
//!   line-number shifts.

use std::path::PathBuf;

use opca_core::task::evidence_gate::{ErrorEntry, ErrorKind, EvidenceGate};
use opca_core::task::normalize_error_msg;
use proptest::prelude::*;

// ── 14.11: Evidence Gate baseline diff ───────────────────────────────────

fn temp_workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// Property: capturing a baseline and immediately verifying on the same
    /// workspace must always succeed (no false positives on identical state).
    #[test]
    fn evidence_gate_identical_workspace_never_trips(
        cmd in prop_oneof![
            Just("true"),
            Just("echo hello"),
            Just("printf 'output\\n'"),
        ],
    ) {
        let dir = temp_workspace();
        let mut gate = EvidenceGate::new(vec![cmd.to_string()]);
        gate.capture_baseline(dir.path()).expect("baseline");
        prop_assert!(
            gate.verify(dir.path()).is_ok(),
            "identical workspace must not trip the gate for cmd={cmd}"
        );
    }

    /// Property: running verify N times on an unchanged workspace always
    /// produces the same result (idempotent).
    #[test]
    fn evidence_gate_verify_is_idempotent(
        repeat in 1u8..5,
    ) {
        let dir = temp_workspace();
        let mut gate = EvidenceGate::new(vec!["true".to_string()]);
        gate.capture_baseline(dir.path()).expect("baseline");

        let first = gate.verify(dir.path()).is_ok();
        for _ in 0..repeat {
            let result = gate.verify(dir.path()).is_ok();
        prop_assert_eq!(result, first, "repeated verify must be consistent");
        }
    }

    /// Property: a workspace with a stable error (same command output) across
    /// baseline and verify must not trip the gate.
    #[test]
    fn evidence_gate_stable_error_does_not_trip(
        error_msg in "[a-z]{1,30}",
    ) {
        let dir = temp_workspace();
        // Write a script that always emits the same error.
        let script = format!("echo 'error: {error_msg}' >&2; exit 1");
        let cmd = format!("sh -c '{script}'");

        let mut gate = EvidenceGate::new(vec![cmd]);
        gate.capture_baseline(dir.path()).expect("baseline");
        // The same error is in baseline and current → no NEW errors.
        prop_assert!(
            gate.verify(dir.path()).is_ok(),
            "stable error must not trip: msg={}",
            error_msg
        );
    }

    /// Property: the baseline correctly masks pre-existing failures — a
    /// NEW error (different message) must always trip.
    #[test]
    fn evidence_gate_new_error_always_trips(
        baseline_msg in "baseline_[a-z]{3,10}",
        new_msg in "new_[a-z]{3,10}",
    ) {
        // We can't easily switch the command between baseline and verify,
        // but we can test the matching logic directly.
        let baseline_entry = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: Some(1),
            kind: ErrorKind::CompileError,
            message: format!("error: {baseline_msg}"),
        };
        let new_entry = ErrorEntry {
            file: Some(PathBuf::from("src/lib.rs")),
            line: Some(2),
            kind: ErrorKind::CompileError,
            message: format!("error: {new_msg}"),
        };

        // The new entry must not match the baseline entry (different message
        // hash after normalization).
        let new_hash = normalize_error_msg(&new_entry.message);
        prop_assert!(
            !std::iter::once(&baseline_entry)
                .map(|e| normalize_error_msg(&e.message))
                .any(|x| x == new_hash),
            "new error must have a different hash: baseline={} new={}",
            baseline_msg,
            new_msg
        );
    }
}

// ── 14.12: Issue signature normalization ─────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// Property: normalize_error_msg is deterministic — same input always
    /// produces the same hash.
    #[test]
    fn normalize_is_deterministic(
        msg in "[a-zA-Z0-9 :._/]{1,100}",
    ) {
        let h1 = normalize_error_msg(&msg);
        let h2 = normalize_error_msg(&msg);
        prop_assert_eq!(h1, h2, "same message must produce same hash");
    }

    /// Property: messages differing only by line numbers produce the same
    /// hash (e.g. `src/lib.rs:42` and `src/lib.rs:67` are equivalent).
    #[test]
    fn normalize_survives_line_number_shifts(
        prefix in "[a-z ]{1,30}",
        line_a in 1u32..1000,
        line_b in 1u32..1000,
        suffix in "[a-z ]{0,30}",
    ) {
        let msg_a = format!("{prefix} src/lib.rs:{line_a} {suffix}");
        let msg_b = format!("{prefix} src/lib.rs:{line_b} {suffix}");
        let h_a = normalize_error_msg(&msg_a);
        let h_b = normalize_error_msg(&msg_b);
        prop_assert_eq!(
            h_a, h_b,
            "line-number-only differences must normalize to the same hash: a={} b={}",
            msg_a, msg_b
        );
    }

    /// Property: messages with different content (beyond line numbers) produce
    /// different hashes with overwhelming probability.
    #[test]
    fn normalize_distinguishes_different_messages(
        word_a in "[a-z]{5,15}",
        word_b in "[a-z]{5,15}",
    ) {
        prop_assume!(word_a != word_b);
        let h_a = normalize_error_msg(&word_a);
        let h_b = normalize_error_msg(&word_b);
        prop_assert_ne!(
            h_a, h_b,
            "different messages should produce different hashes: a={} b={}",
            word_a, word_b
        );
    }

    /// Property: column numbers (`:digit:digit`) are also stripped, not just
    /// line numbers.
    #[test]
    fn normalize_strips_column_numbers(
        base in "[a-z]{1,20}",
        line in 1u32..500,
        col_a in 1u32..100,
        col_b in 1u32..100,
    ) {
        let msg_a = format!("error at src/lib.rs:{line}:{col_a} {base}");
        let msg_b = format!("error at src/lib.rs:{line}:{col_b} {base}");
        let h_a = normalize_error_msg(&msg_a);
        let h_b = normalize_error_msg(&msg_b);
        prop_assert_eq!(
            h_a, h_b,
            "column differences must normalize to the same hash"
        );
    }

    /// Property: empty and single-character messages do not panic.
    #[test]
    fn normalize_handles_edge_cases(
        msg in "(|[a-z]|error|:|::|:1|:1:2)",
    ) {
        // Must not panic.
        let _hash = normalize_error_msg(&msg);
    }

    /// Property: IssueSignature constructed from the same error always has
    /// the same msg_hash (round-trip consistency).
    #[test]
    fn issue_signature_hash_consistent(
        msg in "error[: ]{1,3}[a-z]{3,20}",
    ) {
        let hash = normalize_error_msg(&msg);
        let sig = opca_core::task::IssueSignature {
            file: PathBuf::from("src/test.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: hash,
        };
        // Reconstruct — same input must give same hash.
        let hash2 = normalize_error_msg(&msg);
        let sig2 = opca_core::task::IssueSignature {
            file: PathBuf::from("src/test.rs"),
            kind: ErrorKind::CompileError,
            msg_hash: hash2,
        };
        prop_assert_eq!(sig, sig2, "IssueSignature must be consistent");
    }
}
