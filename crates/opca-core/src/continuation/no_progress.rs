//! No-progress detection — prevents doom loops in continuation chains.
//!
//! See `design.md` §D5 and §R2. The detector uses two complementary
//! heuristics: diff-signature comparison (same files, negligible line
//! changes) and audit-finding repetition (same file + same issue category
//! across multiple iterations).

use std::collections::HashSet;

use crate::audit::Finding;

/// Signature of a diff for comparison purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffSignature {
    /// Set of file paths touched.
    files: HashSet<String>,
    /// Estimated net line changes.
    estimated_lines: usize,
}

/// Signature of an audit finding for repetition detection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FindingSignature {
    /// File/location where the finding was reported.
    file: String,
    /// Derived category (e.g. "`test_failure`", "`missing_impl`").
    category: String,
}

/// Derives a coarse category from an issue description so that semantically
/// similar findings across iterations are grouped together.
///
/// The categorization is intentionally keyword-based — it does not need to
/// be perfect, only stable across iterations reporting the same problem.
fn categorize_issue(issue: &str) -> String {
    let lower = issue.to_lowercase();
    if lower.contains("test") || lower.contains("assert") {
        "test_failure"
    } else if lower.contains("missing")
        || lower.contains("unimplemented")
        || lower.contains("not implemented")
        || lower.contains("todo")
    {
        "missing_impl"
    } else if lower.contains("error") || lower.contains("panic") || lower.contains("crash") {
        "runtime_error"
    } else if lower.contains("type") || lower.contains("mismatch") {
        "type_error"
    } else {
        "other"
    }
    .to_string()
}

/// Detects when consecutive iterations produce no meaningful progress.
///
/// Two heuristics run in parallel:
/// - **Diff heuristic**: empty diffs or substantively identical diffs
///   (same file set, fewer than 5 net line changes) increment the counter.
/// - **Finding heuristic**: the same file + issue category appearing in
///   three consecutive audit reports signals a stuck Task.
pub struct NoProgressDetector {
    /// Maximum consecutive no-progress rounds before signaling.
    threshold: u32,
    /// Current consecutive no-progress count (diff-based).
    consecutive_no_progress: u32,
    /// Previous iteration's diff signature for comparison.
    last_signature: Option<DiffSignature>,
    /// History of finding signatures per iteration for repetition detection.
    finding_history: Vec<HashSet<FindingSignature>>,
}

impl NoProgressDetector {
    /// Creates a detector that triggers after `threshold` consecutive no-progress rounds.
    #[must_use]
    pub const fn new(threshold: u32) -> Self {
        Self {
            threshold,
            consecutive_no_progress: 0,
            last_signature: None,
            finding_history: Vec::new(),
        }
    }

    /// Records a diff and returns `true` if this iteration shows no progress.
    ///
    /// Heuristic: empty diff (zero files) or same file set with fewer than
    /// five net line changes versus the prior iteration both count as
    /// no-progress. Meaningful changes reset the counter.
    pub fn record_iteration(&mut self, diff_files: &[String], estimated_lines: usize) -> bool {
        let current = DiffSignature {
            files: diff_files.iter().cloned().collect(),
            estimated_lines,
        };

        let no_progress = if current.files.is_empty() {
            true
        } else if let Some(ref last) = self.last_signature {
            current.files == last.files
                && current.estimated_lines.abs_diff(last.estimated_lines) < 5
        } else {
            false
        };

        if no_progress {
            self.consecutive_no_progress += 1;
        } else {
            self.consecutive_no_progress = 0;
        }

        self.last_signature = Some(current);
        no_progress
    }

    /// Records audit findings and returns `true` if a repeated finding is detected.
    ///
    /// A finding is "repeated" when the same file + category appears in
    /// three consecutive iterations, indicating the Task is stuck on the
    /// same issue.
    pub fn record_audit_findings(&mut self, findings: &[Finding]) -> bool {
        let current: HashSet<FindingSignature> = findings
            .iter()
            .map(|f| FindingSignature {
                file: f.location.clone(),
                category: categorize_issue(&f.issue),
            })
            .collect();

        self.finding_history.push(current);

        if self.finding_history.len() >= 3 {
            let len = self.finding_history.len();
            let recent = &self.finding_history[len - 3..];
            for sig in &recent[2] {
                if recent[0].contains(sig) && recent[1].contains(sig) {
                    return true;
                }
            }
        }

        false
    }

    /// Returns `true` when consecutive no-progress has reached the threshold.
    #[must_use]
    pub const fn is_no_progress(&self) -> bool {
        self.consecutive_no_progress >= self.threshold
    }

    /// Resets the no-progress counter.
    ///
    /// Called when meaningful progress is detected so that a single
    /// productive iteration clears the doom-loop risk.
    pub const fn reset(&mut self) {
        self.consecutive_no_progress = 0;
    }

    /// Returns the current consecutive no-progress count.
    #[must_use]
    pub const fn consecutive_no_progress(&self) -> u32 {
        self.consecutive_no_progress
    }
}

impl Default for NoProgressDetector {
    /// Default threshold of 2 consecutive no-progress rounds.
    fn default() -> Self {
        Self::new(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Severity;
    use rstest::rstest;

    fn make_finding(location: &str, issue: &str) -> Finding {
        Finding {
            severity: Severity::Warning,
            location: location.to_string(),
            issue: issue.to_string(),
        }
    }

    #[test]
    fn empty_diff_is_no_progress() {
        let mut detector = NoProgressDetector::new(2);
        let no_progress = detector.record_iteration(&[], 0);
        assert!(no_progress);
        assert_eq!(detector.consecutive_no_progress(), 1);
    }

    #[rstest]
    #[case(vec!["a.rs".to_string()], 10, vec!["a.rs".to_string()], 12, true)]
    #[case(vec!["a.rs".to_string()], 10, vec!["a.rs".to_string()], 14, true)]
    #[case(
        vec!["a.rs".to_string()],
        10,
        vec!["a.rs".to_string(), "b.rs".to_string()],
        5,
        false
    )]
    #[case(vec!["a.rs".to_string()], 10, vec!["b.rs".to_string()], 10, false)]
    fn same_files_small_change_is_no_progress(
        #[case] files1: Vec<String>,
        #[case] lines1: usize,
        #[case] files2: Vec<String>,
        #[case] lines2: usize,
        #[case] expected: bool,
    ) {
        let mut detector = NoProgressDetector::new(2);
        detector.record_iteration(&files1, lines1);
        let no_progress = detector.record_iteration(&files2, lines2);
        assert_eq!(no_progress, expected);
    }

    #[test]
    fn new_files_touched_is_progress() {
        let mut detector = NoProgressDetector::new(2);
        detector.record_iteration(&["a.rs".to_string()], 10);
        // Different file set = progress.
        let no_progress = detector.record_iteration(&["b.rs".to_string()], 5);
        assert!(!no_progress);
        assert_eq!(detector.consecutive_no_progress(), 0);
    }

    #[test]
    fn progress_resets_counter() {
        let mut detector = NoProgressDetector::new(2);
        // Two no-progress rounds.
        detector.record_iteration(&[], 0);
        detector.record_iteration(&[], 0);
        assert_eq!(detector.consecutive_no_progress(), 2);
        // Meaningful progress resets.
        detector.record_iteration(&["new.rs".to_string()], 50);
        assert_eq!(detector.consecutive_no_progress(), 0);
    }

    #[test]
    fn threshold_boundary_triggers() {
        let mut detector = NoProgressDetector::new(2);
        detector.record_iteration(&[], 0);
        assert!(!detector.is_no_progress());
        detector.record_iteration(&[], 0);
        assert!(detector.is_no_progress());
    }

    #[test]
    fn repeated_finding_three_iterations() {
        let mut detector = NoProgressDetector::new(2);
        let finding = make_finding("src/auth.rs", "test_login fails");

        // Iterations 1 and 2: not yet repeated.
        assert!(!detector.record_audit_findings(std::slice::from_ref(&finding)));
        assert!(!detector.record_audit_findings(std::slice::from_ref(&finding)));

        // Iteration 3: same finding in 3 consecutive iterations.
        assert!(detector.record_audit_findings(std::slice::from_ref(&finding)));
    }

    #[test]
    fn non_repeated_finding_is_not_flagged() {
        let mut detector = NoProgressDetector::new(2);
        let f1 = make_finding("src/a.rs", "test failure");
        let f2 = make_finding("src/b.rs", "missing implementation");

        assert!(!detector.record_audit_findings(std::slice::from_ref(&f1)));
        assert!(!detector.record_audit_findings(std::slice::from_ref(&f2)));
        assert!(!detector.record_audit_findings(std::slice::from_ref(&f1)));
    }

    #[test]
    fn repeated_finding_same_category_different_wording() {
        let mut detector = NoProgressDetector::new(2);
        // Same file + same category (test_failure) but different wording.
        let f1 = make_finding("src/auth.rs", "test_login assertion failed");
        let f2 = make_finding("src/auth.rs", "test_signup test case error");
        let f3 = make_finding("src/auth.rs", "test_logout is broken");

        assert!(!detector.record_audit_findings(&[f1]));
        assert!(!detector.record_audit_findings(&[f2]));
        // Same file + same category across 3 iterations = repeated.
        assert!(detector.record_audit_findings(&[f3]));
    }

    #[test]
    fn reset_clears_counter() {
        let mut detector = NoProgressDetector::new(2);
        detector.record_iteration(&[], 0);
        detector.record_iteration(&[], 0);
        assert!(detector.is_no_progress());
        detector.reset();
        assert!(!detector.is_no_progress());
    }

    #[test]
    fn default_threshold_is_two() {
        let detector = NoProgressDetector::default();
        let mut d = detector;
        d.record_iteration(&[], 0);
        assert!(!d.is_no_progress());
        d.record_iteration(&[], 0);
        assert!(d.is_no_progress());
    }

    #[test]
    fn first_non_empty_diff_is_progress() {
        let mut detector = NoProgressDetector::new(2);
        let no_progress = detector.record_iteration(&["a.rs".to_string()], 10);
        assert!(!no_progress);
    }

    #[test]
    fn empty_findings_do_not_trigger() {
        let mut detector = NoProgressDetector::new(2);
        assert!(!detector.record_audit_findings(&[]));
        assert!(!detector.record_audit_findings(&[]));
        assert!(!detector.record_audit_findings(&[]));
    }
}
