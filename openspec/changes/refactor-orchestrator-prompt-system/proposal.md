## Why

opca's Task agent loop and orchestrator prompts are minimal "you are an agent, use tools, be thorough" instructions. This causes three concrete failure modes in production:

1. **Self-reported completion is trusted**: `Task::run_turn` transitions to `Delivered` whenever the model emits text without tool calls — no build/test/diagnostics verification is performed. Continuation (just shipped) catches some of this via Audit, but Audit is a downstream safety net; the Task itself should produce evidence before claiming done.
2. **Orchestrator routing depends on string prefix matching** (`OPCA_DISPATCH:`): fragile across providers and languages, already half-refactored in WIP (`real.rs` PrefixDetector), needs to land as a real dispatch tool call.
3. **Audit prompt has no judgment criteria**: only output format is specified — verdict depends entirely on the model's prior training, with no calibration guidance, no severity enum definition, no verdict boundaries.

Compare to omo's Sisyphus protocol: Phase 0 Intent Gate, Phase 1 Codebase Assessment, Phase 2A Exploration, Phase 2B Implementation with delegation, Phase 2C Failure Recovery, Phase 3 Completion with Evidence Gate. Each phase has explicit gates, forbidden lists, and stop conditions in the prompt itself. Borrowing this protocol — adapted to opca's background-first architecture — gives Tasks stronger autonomy without giving up safety.

## What Changes

### P0 — Evidence Gate & Hard Blocks (highest ROI)

- **Evidence Gate before Delivered** (BREAKING for internal flow): `Task::run_turn` MUST run verification (build/test/diagnostics via the project's commands) before emitting `TurnVerdict::Completed`. If verification fails, Task enters a 3-strike retry loop; on exhaustion it transitions to `Stuck` (not `Delivered`). Pre-existing failures are detected by running the same commands on the baseline workspace.
- **Hard Blocks list in Task prompt**: forbidden actions enumerated directly in the prompt — `as any` / `@ts-ignore` / `@ts-expect-error` for TS-adjacent, empty `catch(e) {}`, deleting failing tests to "pass", `expect()` in library code, suppressing type errors, leaving broken state after failures. Same for Rust: `unsafe_code` violation attempts, `.unwrap()` in library code, `#[allow(clippy::...)]` without justification.
- **Audit judgment criteria in prompt**: enumerate severity levels (critical / major / minor / info), confidence bands (high ≥0.8, medium 0.5-0.8, low <0.5), verdict decision tree (Confirmed only when high+no critical findings; NeedsFix when any major/critical; NeedsHumanReview when low confidence; FalsePositive only when claim contradicts diff).
- **Drop `OPCA_DISPATCH:` prefix routing** (BREAKING): replace with a `dispatch_task` tool the Orchestrator calls explicitly. WIP `PrefixDetector` in `real.rs` is removed; routing becomes deterministic on tool calls, not best-effort string matching.

### P1 — Task Phase Protocol & TodoWrite

- **Task Phase Protocol**: Task prompt restructured into named phases mirroring omo's structure but adapted to single-Task scope:
  - **Phase 0 Intent Gate**: classify request (trivial / explicit / exploratory / open-ended / ambiguous); ambiguous → ask one clarifying question before starting.
  - **Phase 1 Codebase Assessment**: sample 2-3 similar files, classify state (disciplined / transitional / legacy / greenfield), record the classification in the first heartbeat.
  - **Phase 2 Execution**: standard tool use loop with Hard Blocks enforced.
  - **Phase 3 Completion**: Evidence Gate before claiming done.
- **TodoWrite as first-class tool**: Tasks MUST create a todo list at start of any multi-step work (3+ steps). Heartbeat Layer 1 includes current `in_progress` todo. Orchestrator surfaces todo state in `/task status`.
- **3-strike rule**: after 3 consecutive failed fix attempts at the same issue, Task transitions to `Stuck` (not silent retry forever) and emits a highlight asking for steering.
- **Orchestrator Clarification Protocol**: when a Task pauses for clarification (`Waiting` state), the Orchestrator surfaces the question to the user via a structured notification (not buried in output stream).

### P2 — Continuation Seed Enrichment & Audit Refinement

- **Continuation prompt seed with budget visibility + retrospective**: each iteration's seed now includes (a) iterations used / limit, (b) cost used / limit, (c) no-progress counter, (d) summary of what prior iterations tried (from Cold Store), (e) the specific failing findings. Task enters iteration N knowing what iterations 1..N-1 attempted.
- **Audit confidence calibration**: Audit Agent prompt gains a self-check step — before emitting verdict, the agent must cite which finding drove the verdict and why. DoneClaim verification expands to actually run claimed test commands and check claimed artifacts exist.

### Sub-agent System (P3, large architectural addition)

- **Sub-Task spawning**: Tasks gain a `dispatch_subtask` tool for delegating focused work to a child Task. Sub-Task inherits parent's FocusContract subset, gets a scoped workspace (typically the parent's workspace, not a fresh one), and reports back via highlight + a `subtask_result` tool call.
- **Sub-Task lifecycle integration**: Sub-Tasks appear in the Task Registry with `parent_task_id` linkage. Parent Task enters `Waiting` while sub-tasks run; Orchestrator aggregates sub-task heartbeats into the parent's heartbeat for unified observability.
- **Delegation limits**: max depth 2 (no sub-sub-sub-tasks), max parallel sub-tasks per parent configurable (default 3). Prevents runaway spawning.

## Capabilities

### New Capabilities

- `prompt-system`: Centralized prompt templates and phase-protocol definitions for Orchestrator, Task, Audit, Continuation, Focus. Replaces ad-hoc inline string literals scattered across `provider/prompts.rs`, `task/task.rs`, `audit/agent.rs`, `continuation/coordinator.rs`, `focus/prompt.rs` with a versioned, documented prompt registry.
- `sub-agent-system`: Task delegation and sub-Task spawning. Parent-child Task relationships, scoped workspaces, delegation depth/parallelism limits, sub-Task result aggregation.

### Modified Capabilities

- `task-lifecycle`: ADD Evidence Gate requirement (build/test/diagnostics before `Delivered`); ADD Context-Completion Gate (Task can pause for clarification via `Waiting`); MODIFY the run loop to enforce 3-strike rule (`Stuck` after 3 failed fixes at same issue).
- `orchestrator-core`: DROP `OPCA_DISPATCH` prefix-based routing (MODIFY the existing "Orchestrator routes user messages" requirement); ADD Clarification Protocol requirement (structured surfacing of Task `Waiting` questions to user); ADD Tone/Communication policy in Orchestrator prompt.
- `audit-agent`: MODIFY Audit prompt requirement to include judgment criteria (severity enum, confidence bands, verdict decision tree); MODIFY DoneClaim verification to actually execute claimed commands.
- `task-context-layering`: MODIFY Layer 1 heartbeat to include current `in_progress` todo item; ADD sub-task heartbeat aggregation rules.

## Impact

### Code

- `crates/opca-core/src/task/run.rs`: major rewrite of `run_turn` (Phase Protocol, Evidence Gate, 3-strike counter)
- `crates/opca-core/src/task/task.rs`: TodoWrite tool registration, Phase 0 classification in first heartbeat
- `crates/opca-core/src/provider/prompts.rs`: explodes in size → split into new `prompt_system/` module tree
- `crates/opca-core/src/audit/agent.rs`: prompt rewrite + DoneClaim verification execution
- `crates/opca-core/src/continuation/coordinator.rs`: enrich `prompt_seed_for` with budget + retrospective
- `crates/opca-core/src/orchestrator/`: drop `OPCA_DISPATCH` routing, add Clarification Protocol handler
- `crates/opca-cli/src/real.rs`: REMOVE WIP `PrefixDetector` (replaced by tool-call dispatch), wire new dispatch tool
- `crates/opca-cli/src/commands.rs`: new `/clarify` and `/subtasks` slash commands
- NEW: `crates/opca-core/src/sub_agent/` module tree
- NEW: `crates/opca-core/src/prompt_system/` module tree

### APIs

- **BREAKING (internal)**: `Orchestrator::dispatch_task` signature changes — `parent_task_id: Option<TaskId>` becomes `parent_task_id: Option<TaskId>, scope: DispatchScope` where `DispatchScope = Full | SubTask { parent_workspace, focus_subset }`.
- **BREAKING (internal)**: `CompletionOutcome::Continue` gains `prior_attempts: Vec<PriorAttemptSummary>` field.
- New public tool registrations: `dispatch_task` (Orchestrator-side), `TodoWrite`, `dispatch_subtask`, `Clarify` (Task-side).

### Dependencies

- No new workspace-level dependencies. All changes use existing `tokio`, `serde`, `thiserror`.

### Configuration

- `.agent/config.toml` gains:
  - `[task]` section: `max_consecutive_failures = 3`, `evidence_commands = ["cargo build", "cargo test --no-run", "cargo clippy"]` (project-overridable)
  - `[sub_agent]` section: `max_depth = 2`, `max_parallel_per_parent = 3`

### Migration

- WIP `PrefixDetector` in `real.rs` (+302 lines) is REMOVED — replaced by `dispatch_task` tool. Users who configured custom dispatch triggers in `[orchestrator] dispatch_prefix` should remove that config key (warning emitted on load if still present, but no crash).
- Existing continuation chains in flight across the upgrade are unaffected (continuation budget/state lives in memory, no on-disk format change).

### Effort

- **2-3 weeks** estimated (matches user-confirmed S3 scope).
- Sequencing: P0 first (Evidence Gate + Hard Blocks + Audit criteria + drop OPCA_DISPATCH) — these are the highest ROI and can ship in week 1. P1 in week 2. Sub-agent system in week 3.
- Each P-level ships as its own commit cluster but lands under one OpenSpec change (single source of truth for the design).
