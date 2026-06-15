# Prompt System

The prompt system is a centralized module tree under
`crates/opca-core/src/prompt_system/` that holds every LLM-facing prompt
template in the project. Each prompt area exposes a `PROMPT_VERSION`
constant so model responses can be correlated with the exact template
that produced them.

## Module layout

```
prompt_system/
  mod.rs                    re-exports, module-level docs
  orchestrator.rs           Orchestrator system prompt + Tone policy
  task/
    mod.rs                  TaskPromptBuilder, phase composition
    phase_0_intent_gate.rs  Phase 0 — Intent Gate instructions
    phase_1_assessment.rs   Phase 1 — Codebase Assessment instructions
    phase_2_execution.rs    Phase 2 — Implementation + Hard Blocks
    phase_3_completion.rs   Phase 3 — Completion + Evidence Gate
    focus.rs                Focus Contract prompt section
  audit/
    mod.rs                  AuditPromptBuilder (dimensions interpolation)
    judgment.rs             Severity definitions + decision tree
  continuation/
    mod.rs                  ContinuationPromptBuilder (re-exports)
    retrospective.rs        Budget + retrospective seed builder
```

## Versioning

Each prompt area exposes a `PROMPT_VERSION` constant:

| Area          | Constant location                              | Current version     |
|---------------|------------------------------------------------|---------------------|
| Orchestrator  | `prompt_system::orchestrator::PROMPT_VERSION`  | `orchestrator-v3`   |
| Task base     | `prompt_system::task::PROMPT_VERSION`           | `task-v1`           |
| Task Phase 0  | `phase_0_intent_gate::PROMPT_VERSION`           | `task-phase0-v1`    |
| Task Phase 1  | `phase_1_assessment::PROMPT_VERSION`            | `task-phase1-v1`    |
| Task Phase 2  | `phase_2_execution::PROMPT_VERSION`             | `task-phase2-v1`    |
| Task Phase 3  | `phase_3_completion::PROMPT_VERSION`            | `task-phase3-v1`    |
| Audit         | `prompt_system::audit::PROMPT_VERSION`          | `audit-v2`          |
| Continuation  | `retrospective::PROMPT_VERSION`                 | `continuation-v2`   |

Bump the version on any material wording change. Whitespace-only edits do
not require a bump. The version is logged at Task/Audit/Orchestrator
initialization in heartbeats.

## Phase protocol

Task prompts are composed cumulatively based on the current phase:

1. **Phase 0 — Intent Gate** (always present): classify the request as
   trivial, explicit, exploratory, open-ended, or ambiguous. If
   ambiguous, call `request_clarification`.
2. **Phase 1 — Codebase Assessment**: sample 2-3 similar files, classify
   project state (disciplined / transitional / legacy / greenfield),
   emit an Assessment highlight.
3. **Phase 2 — Implementation**: make changes using tools. Includes the
   Hard Blocks list (forbidden actions). For 3+ step work, call
   `todowrite` at the start.
4. **Phase 3 — Completion**: the Evidence Gate runs
   (`cargo build`, `cargo test --no-run`, `cargo clippy`). If it passes,
   the Task transitions to Delivered. If it fails, the Task returns to
   Phase 2. After 3 consecutive identical failures, the Task goes Stuck.

Phases are enforced via a hybrid approach: the prompt describes them in
natural language, and the run loop structurally tracks the current phase
and emits `phase-transition` highlights. The loop does not enforce phase
ordering at the type level — the model can revisit earlier phases if
mid-execution ambiguity arises.

## Hard Blocks

The Phase 2 prompt includes a forbidden-actions list that the model is
instructed to never violate:

1. `unsafe` code (forbidden at workspace level)
2. `.unwrap()` in library code
3. `expect()` outside tests
4. Unjustified `#[allow(clippy::...)]`
5. Leaving code in broken state after failures
6. Deleting failing tests to "pass"
7. Shotgun debugging
8. `as Any` type erasure
9. Empty `catch(e) {}` blocks
10. `@ts-ignore` or equivalent type-suppression directives

These are prompt-enforced. `cargo clippy` provides the post-hoc
structural enforcement (`unwrap_used`, `expect_used`, etc.).

## Evidence Gate

Before a Task transitions to Delivered, the configured evidence commands
run on the workspace. A **baseline** is captured at Task dispatch time;
only **new** failures (not in the baseline) block delivery.

Configure via `[task] evidence_commands` in `.agent/config.toml`. Set to
an empty list to disable the gate entirely.

## 3-Strike rule

When the Evidence Gate fails, the failure is hashed into an
`IssueSignature` (file, error kind, normalized message hash). After 3
consecutive failures with the same signature, the Task transitions to
Stuck. Steering injection during Stuck clears the counters and resumes.

## Audit judgment criteria

The Audit prompt includes:

- **Severity definitions**: critical, major, minor, info.
- **Decision tree**:
  - Confidence >= 0.8 AND no critical findings -> Confirmed
  - Confidence < 0.5 -> NeedsHumanReview
  - DoneClaim contradicts diff -> FalsePositive
  - Any major or critical finding -> NeedsFix
  - Otherwise -> Confirmed
- **Justification field**: the Audit JSON must include a `justification`
  string explaining WHY the verdict was chosen, citing specific findings.

## Continuation seed

Each continuation iteration receives a structured seed prompt containing:

1. **Audit Findings** — sanitized findings from the most recent audit.
2. **Budget** — iterations / cost / time consumed vs. limits.
3. **Retrospective** — one-line summary of each prior iteration.
4. **No-Progress Warning** (if applicable) — fires when consecutive
   iterations stall.

## How to add a new prompt section

1. Create a new file under the appropriate subdirectory (e.g.
   `prompt_system/task/phase_5_review.rs`).
2. Define a `pub const &str` with the prompt text.
3. Expose a `PROMPT_VERSION` constant.
4. Compose it into the parent prompt builder (e.g.
   `build_task_prompt(phase)` in `task/mod.rs`).
5. Add snapshot tests using `insta::assert_snapshot!`.
6. Bump the parent area's `PROMPT_VERSION` if the change is material.

## Dispatch routing

The Orchestrator uses the `dispatch_task` tool call (not string-prefix
matching) to route work to background Tasks. The legacy `OPCA_DISPATCH:`
prefix is treated as ordinary text. If `[orchestrator] dispatch_prefix`
is present in the config, a deprecation warning is logged on startup.

## Migration from OPCA_DISPATCH

If you have existing configs using `dispatch_prefix`:

1. Remove the `dispatch_prefix` key from `[orchestrator]` in
   `.agent/config.toml`.
2. No other changes are needed. The Orchestrator's model now uses the
   `dispatch_task` tool automatically.
3. Any text output containing `OPCA_DISPATCH:` is shown to the user as
   ordinary text — it no longer triggers Task dispatch.
