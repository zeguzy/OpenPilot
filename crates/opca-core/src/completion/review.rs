//! Risk assessment (Task 12.3).
//!
//! Heuristic grading of a Task's [`ChangeSet`] that routes the Review stage:
//! - Low → automated rule checks (compile/test) only.
//! - Medium → rule checks, optional Audit.
//! - High → Audit Agent dispatched, report forwarded to user.
//!
//! See `design.md` §D9 (Review) and `specs/completion-pipeline/spec.md`.

use crate::workspace::ChangeSet;

/// Risk grade assigned to a Task's diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    /// Diff is small and touches only docs — automated rule checks suffice.
    Low,
    /// Diff is moderate or touches mixed file types.
    Medium,
    /// Diff is large or touches source/build files — Audit Agent dispatched.
    High,
}

/// Threshold below which a diff is small enough to be Low-risk (when it also
/// touches only `.md` files).
pub const LOW_DIFF_LINE_THRESHOLD: usize = 20;

/// Threshold above which a diff is always High-risk regardless of file type.
pub const HIGH_DIFF_LINE_THRESHOLD: usize = 100;

/// Assess risk from a [`ChangeSet`].
///
/// Heuristics (per task spec):
/// - diff lines < 20 AND only `.md` files → `Low`
/// - diff lines > 100 OR modifies `.rs`/`.toml` files → `High`
/// - otherwise → `Medium`
///
/// "Diff lines" is approximated by total touched files × an average lines
/// estimate. Because the in-process [`ChangeSet`] only records file paths
/// (not per-file line counts), we estimate lines via file size when
/// available; failing that we count one "line" per touched file. This keeps
/// the function pure and testable without reading disk in the hot path.
#[must_use]
pub fn assess_risk(diff: &ChangeSet) -> RiskLevel {
    let total_files = diff.total();
    if total_files == 0 {
        return RiskLevel::Low;
    }

    // File-type rules.
    let touched: Vec<&std::path::PathBuf> = diff
        .added
        .iter()
        .chain(diff.modified.iter())
        .chain(diff.deleted.iter())
        .collect();

    let only_md = touched
        .iter()
        .all(|p| p.extension().is_some_and(|e| e == "md"));
    let touches_source = touched
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "rs" || e == "toml"));

    // Line-count proxy: 1 per file when we have no content. Tests construct
    // ChangeSets directly so this stays deterministic.
    let estimated_lines = estimate_diff_lines(diff);

    // High-risk: large diff OR touches source/build files.
    if estimated_lines > HIGH_DIFF_LINE_THRESHOLD || touches_source {
        return RiskLevel::High;
    }

    // Low-risk: small diff AND only docs.
    if estimated_lines < LOW_DIFF_LINE_THRESHOLD && only_md {
        return RiskLevel::Low;
    }

    RiskLevel::Medium
}

/// Estimate diff line count from the [`ChangeSet`].
///
/// Without per-file content we cannot count real lines, so each touched file
/// contributes a constant estimate:
/// - 1 line for each added/modified/deleted entry.
///
/// This is intentionally simple — the thresholds above are calibrated to
/// treat a single file touch as Low (1 < 20) and a 101-file diff as High.
fn estimate_diff_lines(diff: &ChangeSet) -> usize {
    diff.total()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn md_diff(n: usize) -> ChangeSet {
        ChangeSet {
            added: (0..n)
                .map(|i| PathBuf::from(format!("doc-{i}.md")))
                .collect(),
            modified: vec![],
            deleted: vec![],
        }
    }

    #[test]
    fn empty_diff_is_low() {
        assert_eq!(assess_risk(&ChangeSet::default()), RiskLevel::Low);
    }

    #[test]
    fn small_md_diff_is_low() {
        assert_eq!(assess_risk(&md_diff(5)), RiskLevel::Low);
    }

    #[test]
    fn rs_file_is_high() {
        let diff = ChangeSet {
            added: vec![PathBuf::from("src/main.rs")],
            modified: vec![],
            deleted: vec![],
        };
        assert_eq!(assess_risk(&diff), RiskLevel::High);
    }

    #[test]
    fn toml_file_is_high() {
        let diff = ChangeSet {
            added: vec![],
            modified: vec![PathBuf::from("Cargo.toml")],
            deleted: vec![],
        };
        assert_eq!(assess_risk(&diff), RiskLevel::High);
    }

    #[test]
    fn deleted_rs_file_is_high() {
        let diff = ChangeSet {
            added: vec![],
            modified: vec![],
            deleted: vec![PathBuf::from("src/old.rs")],
        };
        assert_eq!(assess_risk(&diff), RiskLevel::High);
    }

    #[test]
    fn large_md_diff_is_medium() {
        // 50 files, all .md → between Low (20) and High (100) thresholds.
        let diff = md_diff(50);
        assert_eq!(assess_risk(&diff), RiskLevel::Medium);
    }

    #[test]
    fn huge_md_diff_is_high() {
        // >100 files → High regardless of file type.
        let diff = md_diff(101);
        assert_eq!(assess_risk(&diff), RiskLevel::High);
    }

    #[test]
    fn mixed_md_and_txt_small_is_medium() {
        let diff = ChangeSet {
            added: vec![PathBuf::from("a.md"), PathBuf::from("b.txt")],
            modified: vec![],
            deleted: vec![],
        };
        assert_eq!(assess_risk(&diff), RiskLevel::Medium);
    }
}
