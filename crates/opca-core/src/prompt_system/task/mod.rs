//! Task agent system prompt template and phase-composition logic.
//!
//! The base identity (`TASK_SYSTEM`) is always present. Phase sections
//! (0-3) are appended cumulatively based on the Task's current
//! [`Phase`](crate::task::run::Phase) to produce a phase-aware system
//! prompt.
//!
//! See `design.md` §D1 (module structure) and §D2 (phase protocol).

pub mod focus;
pub mod phase_0_intent_gate;
pub mod phase_1_assessment;
pub mod phase_2_execution;
pub mod phase_3_completion;

pub use phase_0_intent_gate::PHASE_0_INSTRUCTIONS;
pub use phase_1_assessment::PHASE_1_INSTRUCTIONS;
pub use phase_2_execution::{HARD_BLOCKS_RUST, PHASE_2_INSTRUCTIONS};
pub use phase_3_completion::PHASE_3_INSTRUCTIONS;

use crate::task::run::Phase;

/// Prompt template version. Bump on any material wording change so
/// consumers can correlate model responses with the exact template.
pub const PROMPT_VERSION: &str = "task-v1";

const TASK_SYSTEM: &str = "\
You are opca, a background code agent worker (Task). \
You work inside an isolated workspace — a copy of the project where you can make changes freely.\n\n\
Your job is to complete the task you've been assigned. Use the available tools \
(read, write, edit, bash, grep, find, ls) to explore the codebase, make changes, \
and verify your work.\n\n\
When you discover something important, use the report_highlight tool to notify the Orchestrator. \
Focus on the dimensions specified in your Focus Contract below.\n\n\
Be thorough but efficient. After completing your work, provide a clear summary of what you did.";

/// Returns the Task agent base identity prompt (without phase sections).
#[must_use]
pub const fn task_prompt() -> &'static str {
    TASK_SYSTEM
}

/// Composes the full Task system prompt for the given phase.
///
/// Phase 0 instructions are always appended. Phase 1, 2 (incl. Hard
/// Blocks), and 3 sections are appended cumulatively as the Task
/// progresses through phases.
#[must_use]
pub fn build_task_prompt(phase: Phase) -> String {
    let mut prompt = format!("{TASK_SYSTEM}\n\n{PHASE_0_INSTRUCTIONS}");

    if matches!(phase, Phase::One | Phase::Two | Phase::Three) {
        prompt.push_str("\n\n");
        prompt.push_str(PHASE_1_INSTRUCTIONS);
    }

    if matches!(phase, Phase::Two | Phase::Three) {
        prompt.push_str("\n\n");
        prompt.push_str(PHASE_2_INSTRUCTIONS);
        prompt.push_str("\n\n");
        prompt.push_str(HARD_BLOCKS_RUST);
    }

    if matches!(phase, Phase::Three) {
        prompt.push_str("\n\n");
        prompt.push_str(PHASE_3_INSTRUCTIONS);
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_zero_prompt_contains_base_and_intent_gate() {
        let prompt = build_task_prompt(Phase::Zero);
        assert!(prompt.contains("background code agent worker"));
        assert!(prompt.contains("Phase 0"));
        assert!(prompt.contains("Intent Gate"));
        assert!(!prompt.contains("## Phase 1"));
    }

    #[test]
    fn phase_one_prompt_contains_assessment() {
        let prompt = build_task_prompt(Phase::One);
        assert!(prompt.contains("Phase 0"));
        assert!(prompt.contains("## Phase 1"));
        assert!(prompt.contains("Codebase Assessment"));
        assert!(!prompt.contains("## Phase 2"));
    }

    #[test]
    fn phase_two_prompt_contains_hard_blocks() {
        let prompt = build_task_prompt(Phase::Two);
        assert!(prompt.contains("## Phase 2"));
        assert!(prompt.contains("Implementation"));
        assert!(prompt.contains("Hard Blocks"));
        assert!(!prompt.contains("## Phase 3"));
    }

    #[test]
    fn phase_two_prompt_contains_todowrite_instructions() {
        let prompt = build_task_prompt(Phase::Two);
        assert!(prompt.contains("TodoWrite"));
        assert!(prompt.contains("todowrite"));
        assert!(prompt.contains("in_progress"));
        assert!(prompt.contains("completed"));
    }

    #[test]
    fn phase_three_prompt_contains_evidence_gate() {
        let prompt = build_task_prompt(Phase::Three);
        assert!(prompt.contains("Phase 3"));
        assert!(prompt.contains("Evidence Gate"));
        assert!(prompt.contains("cargo build"));
    }

    #[test]
    fn hard_blocks_listed_in_phase_two() {
        let prompt = build_task_prompt(Phase::Two);
        assert!(prompt.contains("unsafe"));
        assert!(prompt.contains(".unwrap()"));
        assert!(prompt.contains("expect()"));
        assert!(prompt.contains("#[allow(clippy"));
        assert!(prompt.contains("broken state"));
        assert!(prompt.contains("Deleting failing tests"));
        assert!(prompt.contains("Shotgun debugging"));
        assert!(prompt.contains("as Any"));
        assert!(prompt.contains("catch(e) {}"));
        assert!(prompt.contains("@ts-ignore"));
    }

    #[test]
    fn prompt_version_is_accessible() {
        assert!(PROMPT_VERSION.starts_with("task"));
        assert_eq!(phase_0_intent_gate::PROMPT_VERSION, "task-phase0-v1");
        assert_eq!(phase_1_assessment::PROMPT_VERSION, "task-phase1-v1");
        assert_eq!(phase_2_execution::PROMPT_VERSION, "task-phase2-v1");
        assert_eq!(phase_3_completion::PROMPT_VERSION, "task-phase3-v1");
    }

    #[test]
    fn snapshot_phase_zero_prompt() {
        let prompt = build_task_prompt(Phase::Zero);
        insta::assert_snapshot!("phase_zero_prompt", prompt);
    }

    #[test]
    fn snapshot_phase_one_prompt() {
        let prompt = build_task_prompt(Phase::One);
        insta::assert_snapshot!("phase_one_prompt", prompt);
    }

    #[test]
    fn snapshot_phase_two_prompt() {
        let prompt = build_task_prompt(Phase::Two);
        insta::assert_snapshot!("phase_two_prompt", prompt);
    }

    #[test]
    fn snapshot_phase_three_prompt() {
        let prompt = build_task_prompt(Phase::Three);
        insta::assert_snapshot!("phase_three_prompt", prompt);
    }
}
