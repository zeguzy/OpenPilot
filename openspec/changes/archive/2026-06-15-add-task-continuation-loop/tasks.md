## 1. Phase 0: Prerequisite wiring (fill existing gaps)

- [x] 1.1 Wire `CompletionPipeline::run` into `real.rs` poll_loop: when `collect_changes` detects `TaskStatus::Delivered`, spawn pipeline in isolated tokio task, communicate outcome via channel. Verify: existing e2e tests pass; manual test shows pipeline stages execute on Task completion.
- [x] 1.2 Replace `review_stage` hardcoded `AuditVerdict::Warn` stub: call `AuditAgent::new` with workspace path, diff, and focus for medium/high-risk Tasks. Verify: `grep` confirms no hardcoded verdict in `pipeline.rs`; Audit report unit test passes.
- [x] 1.3 Replace `drain_successors` `tracing::info!` stub with real `dispatch_task` call: when `DependencyGraph::drain_successors` returns successors, dispatch each via Orchestrator. Verify: successor dispatch e2e test confirms new TaskId appears in registry.

## 2. Phase 1: Core continuation module

- [x] 2.1 Create `opca-core/src/continuation/mod.rs` module root with `//!` doc block explaining the continuation loop concept and referencing design.md §D1-D7. Re-export public types from crate root.
- [x] 2.2 Implement `ContinuationBudget` (`budget.rs`): struct with max_iterations, max_total_cost_usd, max_total_duration, max_no_progress_rounds + tracking fields. Methods: `can_continue() -> bool`, `record_iteration(cost, duration)`, `record_no_progress()`, `reset_no_progress()`, `exhausted_dimension() -> Option<BudgetDimension>`. Unit tests for each dimension's exhaustion.
- [x] 2.3 Implement `ContinuationChain` (`chain.rs`): ChainId (newtype), ChainStatus (Active/Terminated(ChainTerminationReason)), IterationRecord, ContinuationChain struct with iterations vec. Methods: `current_iteration()`, `append_iteration()`, `terminate(reason)`. Unit tests for lifecycle transitions.
- [x] 2.4 Implement `ChainTerminationReason` enum: ConfirmedComplete, BudgetExhausted(BudgetDimension), NoProgress, UserCancelled, NeedsHumanReview, TaskError(String). Each variant drives different notification format (verify via snapshot test with `insta`).
- [x] 2.5 Implement `NoProgressDetector` (`no_progress.rs`): tracks consecutive iterations' diff signatures (file set + line change count). Returns `NoProgress` when consecutive_no_progress >= threshold. Heuristic: same file set + <5 net line changes = no progress. Also detects repeated same-finding (same file + same category across 3 iterations). Unit tests with rstest parametric cases.
- [x] 2.6 Define `ContinuationPolicy` trait (`policy.rs`): `decide(&self, audit_report, chain, budget) -> PolicyDecision` where PolicyDecision is Continue(reason) / Terminate(reason). Implement `DefaultContinuationPolicy` following D4 two-layer completion rules. Unit tests for each AuditVerdict → PolicyDecision mapping.
- [x] 2.7 Implement `ContinuationCoordinator` (`coordinator.rs`): holds policy, budget per chain, chain registry. Methods: `start_chain(root_task, config) -> ChainId`, `evaluate(task_id, audit_report) -> Option<DispatchRequest>`, `terminate(chain_id, reason)`. The coordinator is the integration point between pipeline and orchestrator. Unit tests with ScriptedProvider mock.
- [x] 2.8 Add `CompletionOutcome::Continue` variant to `completion/pipeline.rs`: `Continue { reason: ContinuationReason, next_prompt_seed: String, chain_id: ChainId, iteration: u32 }`. Define `ContinuationReason` enum (AuditRejected, TestsFailed, TaskSelfReportedIncomplete, SuccessorActivated). Fix all `match` sites (compiler-enforced exhaustive match).
- [x] 2.9 Refine `AuditVerdict` in `audit/report.rs` from Pass/Warn/Fail to Confirmed/FalsePositive/NeedsFix/NeedsHumanReview. Update all match sites. Map existing usage: `pass→Confirmed`, `fail→NeedsFix or NeedsHumanReview` (by confidence threshold), `warn→FalsePositive or NeedsFix`. Verify `cargo test --workspace` passes.
- [x] 2.10 Add `continuation_stage` to CompletionPipeline runs after cleanup_stage. Invokes `ContinuationCoordinator::evaluate()`. If evaluation returns DispatchRequest, constructs continuation prompt seed (prior audit findings + failing tests + Cold Store recall summary) and returns `CompletionOutcome::Continue`. Verify: pipeline integration test covers the Continue path.
- [x] 2.11 Implement continuation prompt seed construction: structured template that includes iteration number, prior audit findings (sanitized/truncated), failing test names + output, and Cold Store references keyed by chain_id. Sanitization: truncate fields >500 chars, escape control chars. Unit test verifies prompt structure.

## 3. Phase 2: Orchestrator integration

- [x] 3.1 Add `parent_task_id: Option<TaskId>` to `dispatch_task` signature in `orchestrator.rs`. Store in `TaskEntry`. When Some, link new Task to parent in registry. Verify: existing dispatch tests pass with None default.
- [x] 3.2 Add `continuation_chains: HashMap<ChainId, ContinuationChain>` to Orchestrator. Methods: `register_chain()`, `get_chain()`, `list_active_chains()`. Chains registered when ContinuationCoordinator creates them; retained after termination until session ends.
- [x] 3.3 Wire Orchestrator's Delivered detection to pipeline trigger: when poll_loop heartbeat shows Delivered, call `CompletionPipeline::run` in spawned task (this formalizes task 1.1 into the Orchestrator's architecture). Pipeline outcome channel feeds back to Orchestrator for registry updates and continuation dispatch.
- [x] 3.4 Wire ContinuationCoordinator dispatch: when pipeline returns `Continue`, Coordinator constructs DispatchRequest, Orchestrator calls `dispatch_task` with parent_task_id and continuation prompt. Verify: integration test shows chain iteration 2 dispatched after iteration 1 returns Continue.

## 4. Phase 3: CLI and configuration

- [x] 4.1 Add `/continue` slash command to `commands.rs`: accepts prompt string and optional flags (--max-iterations, --budget, --ultrawork). Creates a continuation chain, dispatches first Task. Display chain ID to user.
- [x] 4.2 Add `/stop-continuation` slash command: accepts chain ID (or "all"), terminates specified chain(s) with UserCancelled reason. Currently running Task completes normally but no further iteration dispatches.
- [x] 4.3 Add `/continue status [chain-id]` subcommand: displays chain status, current iteration, budget consumption (iterations used/limit, cost used/limit, duration), and per-iteration summary (task_id, verdict, cost, diff summary).
- [x] 4.4 Add `[continuation]` section to `.agent/config.toml` schema and `docs/configuration.md`: `enabled` (default false), `max_iterations` (default 10), `ultrawork_max_iterations` (default 50), `max_total_cost_usd` (default 5.0), `max_total_duration_minutes` (default 30), `max_no_progress_rounds` (default 2), `audit_confidence_threshold` (default 0.5). Parse and validate in config loader.

## 5. Phase 4: Testing

- [x] 5.1 Unit tests for `continuation/` module: ContinuationBudget exhaustion (all 4 dimensions), ContinuationChain lifecycle, NoProgressDetector (empty diff, identical diff, repeated findings), DefaultContinuationPolicy (each AuditVerdict mapping). Use rstest for parametric cases, proptest for budget invariant (accumulated cost never exceeds max).
- [x] 5.2 Integration test: CompletionPipeline with Continue outcome. ScriptedProvider returns responses that trigger NeedsFix audit → verify Continue outcome produced → verify continuation Task dispatched with correct prompt seed.
- [x] 5.3 E2E test: full continuation chain lifecycle. Script: Task A completes (NeedsFix audit) → iteration 2 dispatched → iteration 2 completes (Confirmed audit) → chain terminates. Verify: chain status transitions Active→Terminated(ConfirmedComplete), 2 IterationRecords, budget correct.
- [x] 5.4 E2E test: budget exhaustion. Script: chain with max_iterations=2, each iteration returns NeedsFix → verify chain terminates after iteration 2 with BudgetExhausted(Iterations), user notification includes iteration count.
- [x] 5.5 E2E test: no-progress detection. Script: 2 consecutive iterations with empty diffs → verify chain terminates with NoProgress before reaching max_iterations.
- [x] 5.6 E2E test: user cancellation. Start chain, run `/stop-continuation` mid-iteration → verify chain terminates with UserCancelled, running Task completes normally.
- [x] 5.7 Snapshot tests (`insta`) for: continuation prompt seed template, chain termination notifications (each ChainTerminationReason), `/continue status` output format. Review with `cargo insta review`.
- [x] 5.8 Proptest: state machine invariant—no continuation iteration ever reuses an existing TaskId; continuation chain iterations are monotonically numbered; terminated chains never dispatch new Tasks.

## 6. Phase 5: Documentation

- [x] 6.1 Update `docs/configuration.md` with `[continuation]` section reference.
- [x] 6.2 Update `docs/getting-started.md` or `docs/architecture.md` with continuation loop concept, the two-layer completion protocol, and `/continue` usage examples.
- [x] 6.3 Add `//!` module doc to `continuation/mod.rs` linking to design.md sections and explaining the Sisyphus metaphor (each iteration is a new boulder push, only Confirmed reaches the summit).
- [x] 6.4 Verify `cargo clippy --workspace --all-targets` is clean and `cargo fmt --all -- --check` passes after all changes.
