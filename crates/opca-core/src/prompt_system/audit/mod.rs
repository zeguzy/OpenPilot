//! Audit Agent system prompt template.
//!
//! Moved verbatim from the inline `build_system_prompt` method on
//! `AuditAgent`; the format string and dimension interpolation live here
//! so audit prompt content is discoverable alongside the other templates.
//!
//! See `design.md` §D8 for the judgment criteria rationale.

pub mod judgment;

pub use judgment::{DECISION_TREE, SEVERITY_DEFINITIONS};

/// Prompt template version. Bump on any material wording change so
/// consumers can correlate model responses with the exact template.
pub const PROMPT_VERSION: &str = "audit-v2";

/// Builds the Audit Agent system prompt.
///
/// `dimensions` are the focus dimensions the agent must check, joined
/// with `", "` and interpolated into the bracketed list.
///
/// The prompt includes severity definitions, a decision tree, and the
/// JSON output schema (which requires a `justification` field).
#[must_use]
pub fn audit_prompt(dimensions: &[String]) -> String {
    let dims = dimensions.join(", ");
    format!(
        "You are an Audit Agent reviewing a completed task's diff. \
         You must check these dimensions: [{dims}].\n\n\
         {SEVERITY_DEFINITIONS}\n\n\
         {DECISION_TREE}\n\n\
         You MUST apply the decision tree above. Do not skip severity classification.\n\n\
         Respond ONLY with a JSON object with fields: \
         verdict (\"confirmed\"|\"false_positive\"|\"needs_fix\"|\"needs_human_review\"), \
         confidence (0.0-1.0), \
         findings (array of {{severity, location, issue}}), \
         summary (string), and \
         justification (string — explain WHY you chose this verdict, citing specific findings \
         by severity and location; do not just restate the summary)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_prompt_contains_dimensions() {
        let dims = vec!["compilation".to_string(), "tests".to_string()];
        let prompt = audit_prompt(&dims);
        assert!(prompt.contains("[compilation, tests]"));
    }

    #[test]
    fn audit_prompt_contains_judgment_criteria() {
        let dims = vec!["compilation".to_string(), "tests".to_string()];
        let prompt = audit_prompt(&dims);
        assert!(prompt.contains("critical"), "missing severity: critical");
        assert!(prompt.contains("major"), "missing severity: major");
        assert!(prompt.contains("minor"), "missing severity: minor");
        assert!(prompt.contains("info"), "missing severity: info");
        assert!(
            prompt.contains("Decision tree"),
            "missing decision tree heading"
        );
        assert!(prompt.contains("NeedsFix"), "missing verdict: NeedsFix");
        assert!(prompt.contains("Confirmed"), "missing verdict: Confirmed");
        assert!(
            prompt.contains("justification"),
            "justification field not in prompt"
        );
    }
}
