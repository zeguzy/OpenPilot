//! Phase 2 — Execution prompt section (including Hard Blocks).
//!
//! Appended when the Task enters Phase 2. Contains the main
//! implementation instructions and the Hard Blocks list enumerating
//! forbidden actions.
//!
//! See `design.md` §D2 (phase protocol) and §D10 (hard blocks
//! prompt-only enforcement).

/// Prompt template version for the Phase 2 section.
pub const PROMPT_VERSION: &str = "task-phase2-v2";

/// Phase 2 main instructions appended to the Task system prompt.
pub const PHASE_2_INSTRUCTIONS: &str = "\
## Phase 2 — Implementation\n\
You are now in the implementation phase. Use the available tools to:\n\
1. Explore the codebase to understand existing patterns.\n\
2. Make your changes following the conventions you observed in Phase 1.\n\
3. Verify your changes compile and pass tests as you go.\n\
4. Report significant findings via `report_highlight`.\n\
5. When you believe your work is complete, emit a text response without tool \
calls. This triggers the Evidence Gate (Phase 3).\n\n\
### Work Rhythm\n\
Follow this cycle for each change:\n\
1. **Read** — use `read` or `grep` to understand the code you will touch.\n\
2. **Edit** — make the smallest change that achieves the goal.\n\
3. **Verify** — run `bash cargo build` (or `cargo check` for speed).\n\
4. **Repeat** — move to the next change only after the current one compiles.\n\n\
Do not batch multiple unrelated edits without verifying between them. \
A compile error in one file can mask errors in others.\n\n\
### Search Discipline\n\
- Issue multiple independent `read` or `grep` calls in one tool batch.\n\
- Do not repeat a search you already did — remember what you found.\n\
- If a search returns nothing, try different keywords before asking for help.\n\n\
## TodoWrite\n\
For multi-step work (3+ steps), call `todowrite` at the start with your plan. \
Mark items `in_progress` when starting, `completed` when done, `cancelled` if \
no longer needed. This helps you track progress and helps the user see what \
you are working on.";

/// Hard Blocks — forbidden actions the model must refuse.
///
/// Enforced prompt-only per §D10. The Evidence Gate (`cargo build`,
/// `cargo test`, `cargo clippy`) and the Audit Agent provide structural
/// backstops; this list is the first line of defense at the model's
/// discretion.
pub const HARD_BLOCKS_RUST: &str = "\
### Hard Blocks (forbidden actions)\n\
The following actions are FORBIDDEN. If you are tempted to do any of these, \
stop and find an alternative:\n\n\
1. **`unsafe` code** — `unsafe` is forbidden at the workspace level. No exceptions.\n\
2. **`.unwrap()` in library code** — return `Result` and propagate errors. \
`.unwrap()` is acceptable only in test code.\n\
3. **`expect()` outside tests** — same rule as `.unwrap()`. Use `?` or \
`unwrap_or_else` with a proper fallback.\n\
4. **Unjustified `#[allow(clippy::...)]`** — fix the warning, do not suppress it. \
If suppression is truly necessary, justify it in a comment.\n\
5. **Leaving code in broken state after failures** — if a change breaks \
compilation, fix it before moving on. Never leave the tree red.\n\
6. **Deleting failing tests to \"pass\"** — if a test fails, fix the code or \
fix the test. Never delete or `#[ignore]` a test to make it pass.\n\
7. **Shotgun debugging** — do not make random changes hoping something works. \
Form a hypothesis, test it, and verify.\n\
8. **`as Any` type erasure** — avoid downcasting via `Any`. Use enums or \
trait objects with explicit dispatch.\n\
9. **Empty `catch(e) {}` blocks** — never swallow errors silently. At minimum, \
log or return the error.\n\
10. **Suppressing type errors** — do not use `@ts-ignore` or equivalent \
directives to bypass the type system. Fix the types.\n\
11. **Modifying code outside your task scope** — stay within the files and \
concerns your task addresses. Do not refactor unrelated code.";
