//! Phase 3 — Completion (Evidence Gate) prompt section.
//!
//! Appended when the Task enters Phase 3. Instructs the model that
//! verification commands will be run and that failures return the
//! Task to Phase 2.
//!
//! See `design.md` §D2 and §D3 for the Evidence Gate baseline logic.

/// Prompt template version for the Phase 3 section.
pub const PROMPT_VERSION: &str = "task-phase3-v1";

/// Phase 3 instructions appended to the Task system prompt.
///
/// The Evidence Gate runs automatically when the model emits a
/// text-only response in Phase 2. The model does not need to run the
/// commands itself — the run loop handles verification.
pub const PHASE_3_INSTRUCTIONS: &str = "\
## Phase 3 — Completion (Evidence Gate)\n\
When you emit a text response without tool calls in Phase 2, the system \
automatically runs the Evidence Gate:\n\
- `cargo build` — must compile without errors.\n\
- `cargo test --no-run` — must compile tests without errors.\n\
- `cargo clippy --workspace --all-targets` — must be warning-free.\n\n\
If the Evidence Gate **passes**, your work is accepted and the Task transitions \
to Delivered. Provide a clear summary of what you did in your final response.\n\n\
If the Evidence Gate **fails**, the failure output is injected into your context \
and you return to Phase 2. Fix the errors and try again. Pre-existing failures \
(detected at Task start) are excluded — only new failures introduced by your \
changes count.\n\n\
After 3 consecutive failures on the same issue, the Task transitions to Stuck.";
