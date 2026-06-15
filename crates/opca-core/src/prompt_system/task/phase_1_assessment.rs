//! Phase 1 — Codebase Assessment prompt section.
//!
//! Appended once the Task enters Phase 1 (after the first tool call).
//! The model samples representative files and classifies the project
//! state, then emits an `assessment` highlight to transition to Phase 2.
//!
//! See `design.md` §D2 for the hybrid enforcement rationale.

/// Prompt template version for the Phase 1 section.
pub const PROMPT_VERSION: &str = "task-phase1-v1";

/// Phase 1 instructions appended to the Task system prompt.
///
/// The model samples 2-3 files similar to the target area, classifies
/// the project's discipline level, and emits the classification via
/// `report_highlight` with tag `assessment`.
pub const PHASE_1_INSTRUCTIONS: &str = "\
## Phase 1 — Codebase Assessment\n\
Before implementing, assess the codebase context:\n\
1. Sample 2-3 files in or near the area you will modify.\n\
2. Classify the project state:\n\
   - **Disciplined** — consistent patterns, tests present, lints clean.\n\
   - **Transitional** — mixed patterns, some tests, some lint debt.\n\
   - **Legacy** — inconsistent patterns, sparse tests, significant lint debt.\n\
   - **Greenfield** — new project, few established patterns yet.\n\
3. Emit your assessment via `report_highlight` with tag `assessment`, \
severity `info`, and a summary like \"state: Disciplined\".\n\n\
This classification informs how conservative your changes should be. \
After emitting the assessment, proceed to Phase 2 (Implementation).";
