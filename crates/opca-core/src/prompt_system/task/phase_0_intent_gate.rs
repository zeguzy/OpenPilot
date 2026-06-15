//! Phase 0 — Intent Gate prompt section.
//!
//! Loaded at Task start. The model classifies the incoming request and
//! decides whether to proceed directly to codebase assessment or pause
//! for clarification (Phase 0 ambiguity handling lands with the
//! Clarification Protocol in G9).
//!
//! See `design.md` §D2 for the hybrid enforcement rationale.

/// Prompt template version for the Phase 0 section.
pub const PROMPT_VERSION: &str = "task-phase0-v1";

/// Phase 0 instructions appended to the Task system prompt.
///
/// The model classifies the request into one of five categories and
/// either asks a single clarifying question (if ambiguous) or proceeds
/// to Phase 1 by emitting its first tool call.
pub const PHASE_0_INSTRUCTIONS: &str = "\
## Phase 0 — Intent Gate\n\
You are starting at Phase 0 (Intent Gate). Before writing any code, classify \
the request into one of:\n\
- **trivial** — a single-step change (rename, typo fix, one-liner).\n\
- **explicit** — the scope and target files are named; proceed directly.\n\
- **exploratory** — you need to read code before planning; proceed to assessment.\n\
- **open-ended** — multiple valid approaches; assess first, then choose.\n\
- **ambiguous** — critical information is missing.\n\n\
If the request is **ambiguous**, ask ONE concise clarifying question and stop. \
Do not begin implementation until the ambiguity is resolved.\n\n\
If the request is anything else, proceed to Phase 1 by sampling the codebase \
with your tools. Your first tool call transitions you out of Phase 0.";
