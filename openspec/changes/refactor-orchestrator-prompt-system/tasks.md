## 1. Prompt System Module Foundation (D1)

- [x] 1.1 Create `crates/opca-core/src/prompt_system/` module tree (mod.rs + orchestrator.rs + task/{mod,phase_0,phase_1,phase_2,phase_3,focus}.rs + audit/{mod,judgment,output_format}.rs + continuation/{mod,retrospective}.rs)
- [x] 1.2 Move `ORCHESTRATOR_SYSTEM` const from `provider/prompts.rs` to `prompt_system/orchestrator.rs`; expose via `orchestrator_prompt() -> &'static str` with `PROMPT_VERSION` constant
- [x] 1.3 Move `TASK_SYSTEM` const from `provider/prompts.rs` to `prompt_system/task/mod.rs`; expose via `task_prompt() -> &'static str` with version
- [x] 1.4 Move Audit prompt builder from `audit/agent.rs:160-168` to `prompt_system/audit/mod.rs`; expose via `audit_prompt(dimensions: &[String]) -> String` with version
- [x] 1.5 Move `build_focus_prompt` from `focus/prompt.rs` to `prompt_system/task/focus.rs`; re-export from `focus/` for backward compat
- [x] 1.6 Move `prompt_seed_for` content builder from `continuation/coordinator.rs:217-241` to `prompt_system/continuation/retrospective.rs`; expose via `continuation_seed(...) -> String`
- [x] 1.7 Add prompt-version logging in Task/Audit/Orchestrator initialization heartbeats (records which prompt template version is in use)
- [x] 1.8 Run `cargo test --workspace` to verify no behavior regression after extraction (all existing tests must pass with prompts served from new module)

## 2. P0: Hard Blocks in Task Prompt (D10)

- [x] 2.1 Write `phase_2_execution::HARD_BLOCKS_RUST` const enumerating: `unsafe` code, `.unwrap()` in library code, `expect()` outside tests, unjustified `#[allow(clippy::...)]`, broken-state-after-failures, deleting failing tests, shotgun debugging, `as Any` type erasure, empty `catch(e) {}`, `@ts-ignore` (TS-adjacent)
- [x] 2.2 Compose Task prompt to include Hard Blocks section after Phase 2 description
- [x] 2.3 Add unit test: rendered Task prompt contains each Hard Block item as a substring
- [x] 2.4 Add unit test: rendered Task prompt has `PROMPT_VERSION` constant exposed for logging

## 3. P0: Evidence Gate (D3)

- [x] 3.1 Create `crates/opca-core/src/task/evidence_gate.rs` with `EvidenceGate` struct, `EvidenceResult`, `EvidenceFailure`, `ErrorKind` types
- [x] 3.2 Implement `EvidenceGate::new(commands: Vec<String>) -> Self`
- [x] 3.3 Implement `EvidenceGate::capture_baseline(workspace: &Path) -> Result<Vec<EvidenceResult>>` — runs commands, parses output, stores results
- [x] 3.4 Implement `EvidenceGate::verify(workspace: &Path) -> Result<(), EvidenceFailure>` — runs commands, diffs against baseline, returns new failures only
- [x] 3.5 Add `Task::evidence_gate: Option<EvidenceGate>` field; populate at `Task::new` based on `[task] evidence_commands` config (None if empty)
- [x] 3.6 Modify `Task::run_turn`: before `TurnVerdict::Completed`, if `evidence_gate.is_some()`, call `verify()`. On `Err`, do not transition to `Delivered`; instead feed failure back into Task context and return to Phase 2
- [x] 3.7 Add `[task] evidence_commands = ["cargo build", "cargo test --no-run", "cargo clippy --workspace --all-targets"]` default to docs/configuration.md
- [x] 3.8 Unit tests: baseline-passes-current-passes; baseline-fails-current-fails-same (no new failures); baseline-passes-current-fails-different (gate trips)
- [x] 3.9 Integration test: ScriptedProvider drives Task to completion; evidence gate injects a failure; Task does NOT transition to Delivered

## 4. P0: Audit Judgment Criteria (D8)

- [x] 4.1 Write `prompt_system/audit/judgment.rs` with severity enum definitions (critical/major/minor/info) and decision tree as a const prompt section
- [x] 4.2 Add `justification: String` field to `AuditReport` struct (separate from `summary`); update `AuditReport` Serialize/Deserialize
- [x] 4.3 Update Audit Agent prompt assembly to include judgment criteria + decision tree in system prompt
- [x] 4.4 Update Audit Agent user prompt to require justification in JSON output
- [x] 4.5 Unit test: rendered Audit prompt contains decision tree text and severity definitions
- [x] 4.6 Snapshot test (insta): Audit report JSON shape with `justification` field
- [x] 4.7 Integration test: Audit rejects a Task with major findings; report's justification cites the specific finding

## 5. P0: Drop OPCA_DISPATCH Prefix Routing (D5)

- [x] 5.1 Add `dispatch_task` tool definition to Orchestrator's tool registry in `RealOrchestrator`
- [x] 5.2 Implement Orchestrator-side handler: when model emits `dispatch_task` tool call, invoke `Orchestrator::dispatch_task(prompt, focus, predecessors)`
- [x] 5.3 Return `{"task_id": "...", "status": "dispatched"}` to the model as tool result
- [x] 5.4 Remove `PrefixDetector` struct and all references from `crates/opca-cli/src/real.rs` (DELETE WIP +302 lines)
- [x] 5.5 Remove `OPCA_DISPATCH` const and routing logic from `provider/prompts.rs` and `orchestrator/` modules
- [x] 5.6 Update `prompt_system/orchestrator.rs` to drop prefix routing instructions; add `dispatch_task` tool usage examples instead
- [x] 5.7 Add deprecation warning on startup if `[orchestrator] dispatch_prefix` present in config (warn but do not crash)
- [x] 5.8 Update `docs/configuration.md` to mark `dispatch_prefix` deprecated
- [x] 5.9 Unit tests: Orchestrator emits `dispatch_task` tool call → Task dispatched; Orchestrator emits text only → no Task dispatched; legacy prefix in text → no Task dispatched (treated as ordinary text)
- [x] 5.10 Integration test: end-to-end dispatch via tool call; verify Orchestrator state and Task launch
- [x] 5.11 Clippy clean: fix 2 deferred clippy warnings in `cli_integration.rs:941` (while_let_loop) and `real.rs:903` (doc_markdown on OPCA_DISPATCH backticks) — they will be removed anyway

## 6. P1: Task Phase Protocol (D2)

- [x] 6.1 Add `Phase` enum to `task/run.rs` (Zero/One/Two/Three)
- [x] 6.2 Add `RunState { current_phase, codebase_assessment: Option<Assessment>, three_strike_counters, ... }` struct to `task/run.rs`
- [x] 6.3 Modify `Task::run` loop: initialize RunState at Phase 0; track phase transitions per D2 logic
- [x] 6.4 Write `prompt_system/task/phase_0_intent_gate.rs` const: Phase 0 instructions (classify request as trivial/explicit/exploratory/open-ended/ambiguous; if ambiguous, ask one question via Clarify tool)
- [x] 6.5 Write `prompt_system/task/phase_1_assessment.rs` const: Phase 1 instructions (sample 2-3 similar files; classify state as disciplined/transitional/legacy/greenfield; emit Assessment highlight)
- [x] 6.6 Write `prompt_system/task/phase_3_completion.rs` const: Phase 3 instructions (run Evidence Gate commands; if pass, summarize; if fail, return to Phase 2)
- [x] 6.7 Modify `Task::build_system_prompt` to compose phase sections based on current phase (Phase 0 sections always present; Phase 1/2/3 sections added as transitions occur)
- [x] 6.8 Emit Layer 2 highlight on every phase transition with tag `phase-transition`
- [x] 6.9 Unit tests: Phase 0→1 on first tool call; Phase 1→2 on Assessment highlight; Phase 2→3 on text-without-tools; Phase 3→2 on evidence failure
- [x] 6.10 Snapshot test: rendered Task prompt at each phase contains the correct section

## 7. P1: TodoWrite Tool

- [x] 7.1 Define `TodoWrite` tool in `task/tool_registry.rs` with parameters: `{todos: [{content, status, priority}]}` matching omo's TodoWrite schema
- [x] 7.2 Implement `TodoWrite` execution: store todos in `Task::todo_list: Vec<TodoItem>` field
- [x] 7.3 Include current `in_progress` todo in Layer 1 heartbeat (per task-context-layering delta)
- [x] 7.4 Add `todo: {total, completed, in_progress}` to Layer 1 heartbeat serialization
- [x] 7.5 Wire Orchestrator to surface todo state in `/task status <id>` slash command output
- [x] 7.6 Update Task prompt Phase 2 section: "For work involving 3+ steps, call TodoWrite at the start"
- [x] 7.7 Unit tests: TodoWrite tool call updates Task::todo_list; Layer 1 heartbeat includes todo state; empty todo for trivial Tasks

## 8. P1: 3-Strike Failure Rule (D4)

- [x] 8.1 Define `IssueSignature { file: PathBuf, kind: ErrorKind, msg_hash: u64 }` in `task/run.rs`
- [x] 8.2 Implement `normalize_error_msg(msg: &str) -> u64` — strip line numbers, paths, hash with FNV-1a
- [x] 8.3 Add `three_strike_counters: HashMap<IssueSignature, u8>` to RunState
- [x] 8.4 On Evidence Gate failure: extract IssueSignatures from failure, increment counters; any counter reaching 3 → transition Task to `Stuck` with reason "3-strike: <sig>"
- [x] 8.5 On SteeringMessage::Inject during Stuck: clear relevant counter
- [x] 8.6 Unit tests: same issue 3 times → Stuck; different issue resets; SteeringMessage clears counter
- [x] 8.7 Integration test: ScriptedProvider emits failing code 3 times → Task transitions to Stuck

## 9. P1: Orchestrator Clarification Protocol + Tone (D6)

- [x] 9.1 Add `Notification::Clarification { task_id, question }` variant to `opca_cli::Notification`
- [x] 9.2 Modify Orchestrator heartbeat polling: when Task is `Waiting` with clarification in heartbeat, emit Clarification notification
- [x] 9.3 Add `/answer <task-id> <text>` slash command in `commands.rs`; forwards as `SteeringMessage::Inject`
- [x] 9.4 Add `[task] clarification_timeout_secs = 300` config; on timeout, Orchestrator emits "Task X timed out waiting, proceeded with <best_guess>" notification
- [x] 9.5 Modify `Task::run` to record `best_guess` in heartbeat when entering `Waiting`; on timeout, auto-resume with best_guess
- [x] 9.6 Write Tone policy section in `prompt_system/orchestrator.rs` (no flattery, no status acks, concise, raise concerns)
- [x] 9.7 Add Context-Completion Gate check in Orchestrator dispatch path: before emitting `dispatch_task`, verify (explicit verb) AND (concrete scope) AND (no pending specialist); else ask clarifying question
- [x] 9.8 Unit tests: Orchestrator detects Waiting Task → emits Clarification; `/answer` forwards SteeringMessage; timeout auto-proceeds
- [x] 9.9 Integration test: ambiguous request → Orchestrator asks clarification (no dispatch); sufficient request → dispatches

## 10. P2: Continuation Seed Enrichment (D9)

- [x] 10.1 Move prompt-seed construction to `prompt_system/continuation/retrospective.rs::build_seed(chain, budget, no_progress_counter) -> String`
- [x] 10.2 Add budget status section: "Budget: N/M iterations used, $X/$Y spent, no-progress: P/Q"
- [x] 10.3 Add retrospective section: summaries of `IterationRecord` from `chain.iterations()`, each with task_id, iteration number, summary, and Audit verdict
- [x] 10.4 Add "Do not repeat these failed approaches" instruction after retrospective
- [x] 10.5 Preserve existing findings section (sanitize_field, max 10) — refactor to use shared helpers
- [x] 10.6 Unit tests: seed contains budget numbers; seed contains prior iteration summaries; seed preserves sanitization
- [x] 10.7 Snapshot test (insta): full seed for a 3-iteration chain

## 11. P3: Sub-Agent System Foundation (D7) — Feature Flagged

- [x] 11.1 Add `[features] sub-agents = []` to `crates/opca-core/Cargo.toml`; gate all sub-agent code behind `#[cfg(feature = "sub-agents")]`
- [x] 11.2 Create `crates/opca-core/src/sub_agent/` module tree (mod.rs + dispatch.rs + workspace.rs + aggregation.rs)
- [x] 11.3 Define `SubTaskScope = Inherited | Isolated` enum
- [x] 11.4 Define `SubTaskResult { task_id, summary, artifacts: Vec<PathBuf>, findings: Vec<Finding> }` struct
- [x] 11.5 Implement `dispatch_subtask` tool: validates depth < max_depth (default 2); validates parent's concurrent subtasks < max_parallel_per_parent (default 3); spawns child Task with parent's workspace (or fresh if Isolated)
- [x] 11.6 Set child Task's `parent_task_id`; parent Task enters `Waiting` after dispatch
- [x] 11.7 On child Task `Delivered`: send SubTaskResult to parent via new `subtask_result_tx` channel; parent transitions out of `Waiting`
- [x] 11.8 On child Task `Stuck`: notify parent with failure context; parent decides retry or self-handle
- [x] 11.9 Sub-task skips Phase 0 and Phase 1 (starts at Phase 2) — modify `Task::run` initialization for sub-tasks

## 12. P3: Sub-Agent Aggregation & Limits

- [x] 12.1 Implement depth tracking: `Task::depth: u8` field; root Tasks depth 0; sub-tasks inherit parent depth + 1
- [x] 12.2 Enforce depth limit in `dispatch_subtask` tool handler
- [x] 12.3 Enforce parallel limit per parent: track `parent.active_subtasks: HashSet<TaskId>`
- [x] 12.4 Sub-task heartbeat aggregation: fold sub-task Layer 1 status into parent's Layer 1 `subtasks` field
- [x] 12.5 Sub-task highlight escalation: severity `major` or `critical` → forward to parent's Layer 2 with `[subtask X]` prefix; `minor`/`info` stay local
- [x] 12.6 Auto-update parent's TodoWrite list on sub-task completion (mark delegated item as `completed`)
- [x] 12.7 Add `[sub_agent] max_depth = 2` and `[sub_agent] max_parallel_per_parent = 3` to docs/configuration.md
- [x] 12.8 Unit tests: depth 3 rejected; parallel limit enforced; slot freed on completion
- [x] 12.9 Integration tests: parent dispatches → waits → child completes → parent resumes; child fails → parent notified; depth limit hit; parallel limit hit

## 13. CLI Commands

- [x] 13.1 Add `/answer <task-id> <text>` slash command to `commands.rs` (Clarification Protocol)
- [x] 13.2 Add `/subtasks [parent-task-id]` slash command to list active sub-tasks of a parent (behind `sub-agents` feature flag)
- [x] 13.3 Update `HELP_TEXT` to document new commands
- [x] 13.4 Render Clarification notification as visible banner in REPL (`repl.rs`)
- [x] 13.5 Render sub-task status in `/task status <id>` output when sub-agents feature is enabled

## 14. End-to-End Tests & Validation

- [x] 14.1 E2E: Task with Evidence Gate passing → Delivered normally
- [x] 14.2 E2E: Task with Evidence Gate failing on attempt 1 → 3-strike counter; succeeding on attempt 2 → Delivered
- [x] 14.3 E2E: Task with Evidence Gate failing 3 times same issue → Stuck
- [x] 14.4 E2E: Task with ambiguous request → Phase 0 emits clarification → Waiting → user answers → resumes → Delivered
- [x] 14.5 E2E: User dispatches via Orchestrator's `dispatch_task` tool call → Task launched
- [x] 14.6 E2E: Legacy `OPCA_DISPATCH:` text in Orchestrator output → no Task dispatched
- [x] 14.7 E2E (feature-flagged): Parent Task dispatches sub-task → waits → sub-task completes → parent resumes
- [x] 14.8 E2E (feature-flagged): Sub-task depth limit rejected
- [x] 14.9 E2E (feature-flagged): Sub-task parallel limit rejected
- [x] 14.10 E2E: Continuation seed for iteration 3 contains budget, retrospective, and findings
- [x] 14.11 Property test (proptest): Evidence Gate baseline diff is sound (no false positives on identical runs)
- [x] 14.12 Property test (proptest): Issue signature normalization is deterministic and survives line-number shifts
- [x] 14.13 Snapshot tests (insta): all prompt templates at v1; audit report with justification field

## 15. Documentation & Migration

- [x] 15.1 Update `docs/configuration.md` with new sections: `[task]` (evidence_commands, max_consecutive_failures, clarification_timeout_secs), `[sub_agent]` (max_depth, max_parallel_per_parent)
- [x] 15.2 Mark `[orchestrator] dispatch_prefix` as deprecated in `docs/configuration.md`
- [x] 15.3 Update `AGENTS.md` "Coding conventions" section to reference Hard Blocks (cross-link from prompt)
- [x] 15.4 Add `docs/prompt-system.md` describing the new prompt module tree, versioning policy, and how to add a new prompt section
- [x] 15.5 Write migration notes in `openspec/changes/refactor-orchestrator-prompt-system/proposal.md` (already drafted) — verify accuracy against final implementation
- [x] 15.6 Run `cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace` clean
- [x] 15.7 Manual smoke test: launch `opca-cli`, dispatch a Task, observe Phase transitions in heartbeats, observe Evidence Gate behavior, observe Clarification Protocol
- [x] 15.8 Manual smoke test (with `--features sub-agents`): dispatch a Task that delegates to a sub-task, observe parent Waiting, sub-task progress, parent resumption
