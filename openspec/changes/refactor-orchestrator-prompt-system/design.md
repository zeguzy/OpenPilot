## Context

opca's Task agent loop and prompts were designed for a "minimum viable agent" target. The Task prompt is 9 lines of generic instruction; the Orchestrator prompt uses an `OPCA_DISPATCH:` string prefix hack for routing; the Audit prompt specifies output format but not judgment criteria. In parallel, the recently shipped `add-task-continuation-loop` change introduced a downstream safety net (continuation chains verify Audit verdicts and iterate), but the **upstream** problem remains: Tasks self-report completion without producing evidence.

The WIP `PrefixDetector` refactor in `crates/opca-cli/src/real.rs` (+302 lines, uncommitted) was an intermediate attempt to make `OPCA_DISPATCH` routing more robust by streaming prefix detection rather than line-buffer matching. This design supersedes that refactor: the routing problem is solved structurally by replacing string-prefix dispatch with an explicit `dispatch_task` tool call. The `PrefixDetector` code should be removed.

The omo Sisyphus protocol (used by this very agent) is the reference for phase-based prompt structure. Its key insight: a Task's prompt is not just "you are an agent" — it is a **structured operating procedure** with named phases, gates, forbidden lists, and stop conditions. Borrowing this structure into opca is the central design move.

**Stakeholders:**
- **Tasks** — gain stronger autonomy via phase protocol; lose "freestyle" mode
- **Orchestrator** — gains deterministic dispatch via tool calls; loses fragile prefix matching
- **Audit Agent** — gains calibration; loses freeform judgment
- **Users** — see fewer false-positive `Delivered` claims; gain `/answer` and `/subtasks` commands

## Goals / Non-Goals

**Goals:**
- Eliminate false-positive `Delivered`: Tasks MUST produce evidence before claiming done
- Eliminate `OPCA_DISPATCH` prefix fragility: dispatch becomes a tool call
- Give Audit Agent reproducible judgment criteria in the prompt
- Introduce Task phase protocol (Plan/Assess/Execute/Verify) without breaking existing Tasks
- Introduce sub-task delegation with workspace reuse and depth/parallelism limits
- Co-locate all prompt templates under `prompt_system/` module tree with versioning
- Keep changes non-breaking for end users (CLI surface unchanged except for new commands)

**Non-Goals:**
- **NOT** rewriting the agent loop from scratch — `Task::run_turn` evolves incrementally with new phases and gates; the core loop structure stays
- **NOT** introducing new providers or model integrations — existing Anthropic/OpenAI/Gemini wiring is unchanged
- **NOT** reworking the Audit Agent's verdict taxonomy (already done in continuation change)
- **NOT** building a new memory subsystem — reuses `Memory<T>` for todo lists and phase state
- **NOT** changing workspace isolation strategy — sub-tasks reuse parent workspaces; no new isolation modes
- **NOT** adding new external dependencies — all changes use existing tokio/serde/thiserror/etc.

## Decisions

### D1: Prompt module structure (flat files vs hierarchical module tree)

**Choice:** Hierarchical module tree under `crates/opca-core/src/prompt_system/`:

```
prompt_system/
  mod.rs                    // re-exports, PromptVersion registry
  orchestrator.rs           // Orchestrator prompt + Tone policy
  task/
    mod.rs                  // TaskPromptBuilder, Phase concatenation
    phase_0_intent_gate.rs  // Phase 0 prompt section
    phase_1_assessment.rs   // Phase 1 prompt section
    phase_2_execution.rs    // Phase 2 prompt section (incl. Hard Blocks)
    phase_3_completion.rs   // Phase 3 prompt section (incl. Evidence Gate)
    focus.rs                // Focus Contract prompt section
  audit/
    mod.rs                  // AuditPromptBuilder
    judgment.rs             // Severity/confidence/decision-tree section
    output_format.rs        // JSON output schema section
  continuation/
    mod.rs                  // ContinuationPromptBuilder
    retrospective.rs        // Budget + retrospective section
```

**Rationale:** Each prompt section is a `pub const &str` exposed via accessor functions. Sections compose via `format!` at build time. This allows A/B testing individual sections (swap `phase_2_execution::V1` with `V2`) and per-section version bumps.

**Alternatives considered:**
- *Single `prompts.rs` flat file:* Rejected. Already too long at `prompts.rs` (29 lines today, will be 500+). Hurts readability.
- *External text files (`.md`/`.txt`):* Rejected. Loses compile-time validation of variable interpolation; harder to ship in a binary.
- *Templating engine (`tera`/`handlebars`):* Rejected for now. Adds a dependency; `format!` + `Cow<str>` is sufficient for the substitution complexity we need.

### D2: Phase Protocol enforcement (prompt-only vs structural)

**Choice:** **Hybrid enforcement** — the prompt describes phases in natural language; the run loop structurally tracks `current_phase: Phase` and emits phase-transition Layer 2 highlights. The loop does NOT enforce phase ordering at the type level (e.g., a `Phase0<'a>` newtype that only transitions to `Phase1<'a>`). Reasoning: phases are a *prompting* aid, not a *correctness* invariant.

```rust
// In task/run.rs
enum Phase { Zero, One, Two, Three }

struct RunState {
    current_phase: Phase,
    codebase_assessment: Option<Assessment>,
    three_strike_counters: HashMap<IssueSignature, u8>,
    // ...
}
```

The loop:
1. First turn → always Phase 0. Model classifies request. If "ambiguous" → emit clarification, set state to `Waiting`.
2. Phase 0 → Phase 1: when model emits first tool call (assessment action).
3. Phase 1 → Phase 2: when model emits an `Assessment` highlight (or after 1 turn with no assessment action — fallback).
4. Phase 2 → Phase 3: when model emits text-without-tools AND Evidence Gate is pending.
5. Phase 3: run evidence commands. Pass → `Delivered`. Fail → feed errors back, increment 3-strike counter for the relevant issue signature, return to Phase 2.

**Rationale:** Strict type-level phase ordering is overkill — the model needs freedom to skip Phase 1 if the codebase is well-known, or to revisit Phase 0 if mid-execution ambiguity arises. Hybrid enforcement gives visibility (highlights) without rigidity.

**Alternatives considered:**
- *Prompt-only (no structural tracking):* Rejected. Loses observability and prevents 3-strike counter integration.
- *Type-state pattern (compile-time phase ordering):* Rejected. Over-engineered; restricts legitimate mid-task re-planning.

### D3: Evidence Gate baseline detection

**Choice:** Before running evidence commands on the Task's workspace, the system runs the same commands on a **baseline snapshot** (taken at Task dispatch time, stored as `Option<EvidenceResult>`). Failures that match the baseline (same test names, same error codes) are pre-existing and excluded from the gate. Failures that are new (introduced by the Task's diff) trigger the 3-strike loop.

**Implementation:**
```rust
struct EvidenceGate {
    commands: Vec<String>,                  // configured evidence commands
    baseline: Option<Vec<EvidenceResult>>,  // run at dispatch time
}

impl EvidenceGate {
    fn verify(&self, workspace: &Path) -> Result<(), EvidenceFailure> {
        let current = run_commands(&self.commands, workspace)?;
        let new_failures = diff_against_baseline(&current, &self.baseline);
        if new_failures.is_empty() { Ok(()) } else { Err(EvidenceFailure(new_failures)) }
    }
}
```

Baseline is captured once per Task (at dispatch), reused across all 3 strike attempts. Stored in `Task::evidence_gate` field.

**Rationale:** Pre-existing failures are common in legacy codebases. Without baseline detection, Tasks would `Stuck` immediately on Tasks that touch a module with pre-existing test failures.

**Alternatives considered:**
- *No baseline (treat all failures as new):* Rejected. UX-hostile in real codebases.
- *Run baseline on every attempt:* Rejected. 2x cost per attempt; baseline doesn't change between attempts anyway.
- *Diff-based failure attribution (analyze source changes vs test changes):* Rejected. Too brittle — a change in `src/lib.rs` could legitimately affect tests in `tests/foo.rs`.

### D4: 3-strike issue signature

**Choice:** Issue signature = `(file_path, error_kind, normalized_message_hash)`. `error_kind` is a small enum (`TypeMismatch`/`CompileError`/`TestFailure`/`LintWarning`/`Other`). `normalized_message_hash` is a u64 hash of the error message with numbers/paths stripped (so "expected `usize`, got `i32` at src/lib.rs:42" matches "expected `usize`, got `i32` at src/lib.rs:67").

```rust
struct IssueSignature {
    file: PathBuf,
    kind: ErrorKind,
    msg_hash: u64,
}
```

The Task maintains `HashMap<IssueSignature, u8>` counting consecutive failures. Hitting 3 for any signature → transition to `Stuck`.

**Rationale:** A strict "(file, kind)" key would conflate unrelated failures in the same file. A loose "(kind)" key would under-count. The hash-with-paths-stripped is a middle ground that survives line-number shifts.

**Alternatives considered:**
- *Structural AST diff of attempts:* Rejected. Too complex for marginal benefit.
- *LLM-as-judge ("is this the same issue as last time?"):* Rejected. Expensive and non-deterministic.

### D5: `dispatch_task` tool interface

**Choice:** The Orchestrator gains a registered tool `dispatch_task`:

```rust
ToolDef {
    name: "dispatch_task",
    description: "Dispatch a background Task to handle long-running work. Use when the user's request involves multi-step implementation, refactoring, or research that would block the conversation. Do NOT use for quick answers, lookups, or clarifications.",
    parameters: json!({
        "type": "object",
        "properties": {
            "prompt": {"type": "string", "description": "The full prompt for the Task"},
            "focus": {"type": "array", "items": {"type": "string"}, "description": "Focus dimensions for the Task to monitor"},
            "predecessors": {"type": "array", "items": {"type": "string"}, "description": "Task IDs this Task depends on", "default": []}
        },
        "required": ["prompt"]
    }),
    effects: ToolEffects::Process,
}
```

When the Orchestrator's model emits this tool call, `RealOrchestrator` invokes `Orchestrator::dispatch_task` directly (no string parsing). The result returned to the model is `{"task_id": "...", "status": "dispatched"}`.

**Rationale:** Tool calls are deterministic, schema-validated, and provider-agnostic. Drops the entire `PrefixDetector` complexity.

**Migration:** The `[orchestrator] dispatch_prefix = "OPCA_DISPATCH:"` config key is silently ignored with a one-time warning logged on startup: "config key `dispatch_prefix` is deprecated and ignored; routing uses the `dispatch_task` tool". No crash, no removal of the key from existing configs.

### D6: Clarification Protocol surface

**Choice:** When a Task transitions to `Waiting` with a clarification in its heartbeat, the Orchestrator:
1. Detects the transition via heartbeat polling
2. Emits a structured notification to the CLI via a new `Notification::Clarification { task_id, question }` variant
3. The CLI renders this as a visible banner in the REPL: `[Task A waiting] Question: <text>. Reply with /answer <task-id> <response>`
4. The user types `/answer task-a <text>`; the Orchestrator forwards as `SteeringMessage::Inject`

**Timeout:** 5 minutes default, configurable via `[task] clarification_timeout_secs = 300`. On timeout, the Task auto-proceeds with its recorded best-guess (stored in the heartbeat's `best_guess` field). The Orchestrator emits a follow-up notification: `[Task A timed out waiting] proceeded with: <best_guess>`.

**Rationale:** Existing systems bury clarifications in the output stream — users miss them. A dedicated banner + slash command makes the interaction loop tight.

### D7: Sub-agent workspace strategy

**Choice:** Sub-tasks **inherit the parent's workspace by default**. This is the only viable option for performance: spawning a fresh git worktree per sub-task would dominate delegation cost (worktree creation is ~100ms even with CoW). The trade-off is that sub-tasks can interfere with each other if they touch the same files — mitigated by the parallel limit (3 concurrent sub-tasks default) and by the Focus Contract subset (which scopes what each sub-task is supposed to touch).

For genuine isolation needs, the parent passes `scope = "isolated"` to get a fresh worktree from main branch. On completion, the isolated sub-task's changes merge back to the parent's workspace via the standard `Workspace::merge_into` flow.

**Why not always-isolated:** 100ms × 3 sub-tasks × 5 parents × 10 hops = 15 seconds of pure worktree overhead per Task. Plus merge conflicts on every sub-task completion. Inheriting the parent workspace is the right default.

**Why not always-shared:** Some sub-tasks genuinely need isolation (destructive operations, conflicting file edits). The `scope` parameter is the escape hatch.

### D8: Audit prompt judgment criteria enforcement

**Choice:** The judgment criteria are part of the system prompt (not the user prompt). The Audit Agent is given a fixed system prompt that includes the decision tree; the user prompt contains only the diff and DoneClaim. The output JSON schema gains a `justification` field (separate from `summary`) — the model must cite which finding(s) drove the verdict.

**Decision tree in prompt:**
```
VERDICT DECISION TREE:
- IF confidence >= 0.8 AND no critical findings → Confirmed
- IF confidence < 0.5 → NeedsHumanReview
- IF DoneClaim contradicts diff (claimed artifact missing, claimed command failed) → FalsePositive
- IF any major or critical finding → NeedsFix
- OTHERWISE → Confirmed
```

**Rationale:** The system prompt is loaded once per audit call and not billed per-token in the same way as user prompts. The fixed decision tree eliminates the model's reliance on prior training for verdict calibration.

### D9: Continuation retrospective retrieval

**Choice:** Retrospective content for the continuation prompt seed is retrieved from two sources:
1. **Iteration history** (in-memory `ContinuationChain::iterations: Vec<IterationRecord>`): the prior iterations' summaries and Audit findings. Always available.
2. **Cold Store** (persistent): if the chain spans multiple sessions, additional context (e.g., what configs were tried) is retrieved via the standard `recall` API. Best-effort; absence is OK.

The seed is constructed in `ContinuationCoordinator::prompt_seed_for` and includes:
- Budget status (from `ContinuationBudget`)
- No-progress counter (from `NoProgressDetector`)
- Summaries of prior iterations (from `IterationRecord::summary`)
- Most recent findings (from most recent `IterationRecord::verdict`)

**Rationale:** In-memory chain state is always available; Cold Store is enhancement. This keeps the happy path fast and the persistence layer optional.

### D10: Hard Blocks enforcement (prompt vs post-hoc check)

**Choice:** **Prompt-only** for the first iteration. The Hard Blocks are listed in the Task prompt; the model is expected to refuse. We do NOT add post-hoc AST scanning to detect violations (e.g., grep for `.unwrap()` in library code). The Evidence Gate (D3) catches type-level and test failures; the Audit Agent catches semantic violations; the Hard Blocks list in the prompt is a third line of defense at the model's discretion.

**Rationale:** Post-hoc scanning would duplicate what `cargo clippy` already does (`unwrap_used`, `expect_used`, `as_conversions` lint groups). Recommending project-level clippy config achieves the same outcome with less duplication. Prompt enforcement catches violations before they're written.

**Future:** If we observe Hard Blocks being ignored in practice (Audit findings show `.unwrap()` slipping through), we can revisit with a clippy-driven post-hoc check.

## Risks / Trade-offs

- **[Risk] Phase Protocol adds turn overhead.** Phase 0 + Phase 1 may consume 1-2 extra turns before productive work begins. → *Mitigation:* Phases are skipped if the model emits a tool call immediately (Phase 0 → Phase 2 directly for "trivial" classifications). Cost is bounded to 1 extra turn in the worst case.

- **[Risk] Evidence Gate cost.** Running `cargo build && cargo test --no-run && cargo clippy` per `Delivered` attempt adds 30-90 seconds per Task. → *Mitigation:* Baseline is run once at dispatch (not per attempt). The gate runs only on the final turn (not every turn). For Tasks that legitimately don't touch code (research, docs), `[task] evidence_commands = []` disables the gate.

- **[Risk] `dispatch_task` tool may be over-used by model.** The Orchestrator's model might dispatch Tasks for trivial requests that should be answered inline. → *Mitigation:* The tool description includes "Do NOT use for quick answers, lookups, or clarifications". Additionally, the Orchestrator's system prompt gains examples of when NOT to dispatch (mirroring omo's Phase 0 intent gate examples).

- **[Risk] Sub-agent system adds concurrency complexity.** Parent Tasks in `Waiting` while sub-tasks run; sub-task failures propagate to parent; depth/parallel limits enforced. Race conditions possible. → *Mitigation:* Reuse the existing per-task mpsc channel pattern (proven in continuation work). Sub-task channels are children of parent's channels; no new concurrency primitives introduced. Tests cover parent-waits-while-subtask-runs and subtask-failure-propagates scenarios.

- **[Risk] Audit criteria may be too rigid.** The decision tree (Confirmed requires confidence ≥0.8) may force `NeedsFix` on Tasks that are actually fine but have low-confidence audits. → *Mitigation:* The `confidence_threshold` is configurable (already exists from continuation work, default 0.5). Orchestrator override remains the escape hatch.

- **[Risk] Tone policy enforcement is best-effort.** The model may ignore prompt instructions and emit "Great question!" anyway. → *Mitigation:* Accept this as a known limitation. Post-hoc text filtering (regex replace) is a future option but not in scope.

- **[Risk] Migration breaks configs using `dispatch_prefix`.** Users with custom prefix configs will see silent behavior change. → *Mitigation:* Warning logged on startup if config key is present. Documented in `docs/configuration.md` changelog. No crash, no removal.

- **[Trade-off] Sub-task workspace inheritance vs isolation.** We chose inheritance (default) for performance, accepting the risk of concurrent file edits. → *Acceptance:* The parallel limit (3) and Focus Contract subset scoping make concurrent edits unlikely. Users who need isolation can pass `scope = "isolated"` explicitly.

- **[Trade-off] Issue signature hash is heuristic.** The 3-strike counter uses a hash of normalized error messages; edge cases will over- or under-count. → *Acceptance:* 3-strike is a guard rail, not a precision instrument. Better to slightly under-count (Task retries once more) than to over-count (Task gives up too early).

## Migration Plan

### Pre-deploy
1. Land Phase 0 (prompt-system module creation, no behavior change). Verify all tests pass with prompts served from new module.
2. Land Phase 1 (Hard Blocks + Phase Protocol in Task prompt; dispatch_task tool). Audit existing Tasks' behavior; expect minor latency increase.
3. Land Phase 2 (Evidence Gate, 3-strike, Clarification Protocol). Monitor for spurious `Stuck` transitions.
4. Land Phase 3 (Sub-agent system). Behind a feature flag initially (`--features sub-agents`); default off; flip to default on after 1 week of internal use.

### Deploy
1. Tag release with migration notes.
2. On startup, if `[orchestrator] dispatch_prefix` is present in config, log deprecation warning.
3. Existing in-flight continuation chains complete normally (in-memory state, no format change).

### Rollback
1. Revert to previous release.
2. Continued Tasks from the new release are still valid (no on-disk format changes).
3. Sub-agent feature flag can be disabled independently if sub-agent system has issues.

## Open Questions

1. **Phase 0 ambiguity detection threshold.** The model classifies requests as "trivial/explicit/exploratory/open-ended/ambiguous". What's the right balance? If too sensitive, every request pauses for clarification; if too lax, ambiguous requests dispatch without enough context. → *Resolution:* Start permissive (only pause on explicit "I don't know" or "could be X or Y" language). Tune based on real usage.

2. **Sub-task and continuation interaction.** Can a sub-task be part of a continuation chain? E.g., Task A dispatches sub-task B; B is `NeedsFix` by Audit; does B iterate, or does the failure bubble to A? → *Resolution (proposed):* Sub-tasks do NOT participate in continuation chains. A sub-task's `NeedsFix` Audit verdict bubbles to the parent; the parent decides to re-dispatch a new sub-task or self-handle. Simplifies the state space. Revisit if real usage demands nested continuation.

3. **Evidence Gate for non-Rust projects.** The default evidence commands (`cargo build && cargo test --no-run && cargo clippy`) are Rust-specific. How do we auto-detect project type? → *Resolution (proposed):* Sniff for `package.json` (Node), `requirements.txt`/`pyproject.toml` (Python), `go.mod` (Go), `Cargo.toml` (Rust). Each maps to default evidence commands. User can override via `[task] evidence_commands`. Detection logic lives in `WorkspaceManager` (already does project-type detection for isolation strategy).

4. **Cold Store retrieval in continuation seed.** Is Cold Store mature enough to reliably retrieve "what configs were tried in prior iterations"? → *Defer:* Use in-memory `IterationRecord` only for V1. Enhance with Cold Store recall in V2 if needed.

5. **Audit prompt versioning for A/B testing.** How do we run two Audit prompts in parallel to compare verdict quality? → *Defer:* Out of scope for this change. Versioning infrastructure lands now; A/B mechanism follows.

6. **Sub-task prompt seed for non-Rust projects.** Hard Blocks list is Rust-focused. How do we make it project-aware? → *Resolution:* Same sniffing as Evidence Gate (Q3). Project-specific Hard Blocks lists maintained per language.
