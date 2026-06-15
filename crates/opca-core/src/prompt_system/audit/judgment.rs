//! Audit judgment criteria — severity definitions and decision tree.
//!
//! See `design.md` §D8 for the rationale behind prompt-based judgment
//! calibration. These const sections are composed into the Audit Agent
//! system prompt so the model applies a fixed decision tree rather than
//! relying solely on its own calibration.

/// Prompt template version for the judgment section.
pub const PROMPT_VERSION: &str = "audit-judgment-v1";

/// Severity definitions the Audit Agent must use when classifying findings.
///
/// Each severity maps to a human-readable description so the model
/// applies a consistent threshold across audits.
pub const SEVERITY_DEFINITIONS: &str = r"Severity definitions for findings:
- critical: Blocks merge. Compilation errors, security vulnerabilities, data loss risks, broken core invariants.
- major: Should block merge. Test failures, logic errors in non-trivial paths, missing error handling.
- minor: Worth fixing but not blocking. Style issues, dead code, suboptimal patterns.
- info: Observations. Could be improved. No action required.";

/// Decision tree the Audit Agent must apply to derive a verdict from findings.
///
/// Rules are applied top-down; the first match wins.
pub const DECISION_TREE: &str = r"Decision tree (apply top-down, first match wins):
1. IF any critical finding → verdict = NeedsHumanReview, confidence ≥ 0.9
2. IF any major finding → verdict = NeedsFix, confidence ≥ 0.7
3. IF ≥3 minor findings → verdict = NeedsFix, confidence ≥ 0.6
4. IF only info/minor findings → verdict = Confirmed, confidence ≥ 0.5
5. IF diff is empty or trivial (whitespace, comments only) → verdict = FalsePositive, confidence ≥ 0.8

Verdict definitions:
- Confirmed: Work is correct and complete. Ready to merge.
- FalsePositive: Task claims work that doesn't appear in diff. Nothing meaningful was done.
- NeedsFix: Real issues found. Task must iterate.
- NeedsHumanReview: Cannot determine automatically. Escalate.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_definitions_contains_all_levels() {
        assert!(SEVERITY_DEFINITIONS.contains("critical"));
        assert!(SEVERITY_DEFINITIONS.contains("major"));
        assert!(SEVERITY_DEFINITIONS.contains("minor"));
        assert!(SEVERITY_DEFINITIONS.contains("info"));
    }

    #[test]
    fn decision_tree_contains_verdicts_and_keywords() {
        assert!(DECISION_TREE.contains("critical"));
        assert!(DECISION_TREE.contains("major"));
        assert!(DECISION_TREE.contains("minor"));
        assert!(DECISION_TREE.contains("info"));
        assert!(DECISION_TREE.contains("NeedsFix"));
        assert!(DECISION_TREE.contains("Confirmed"));
    }
}
