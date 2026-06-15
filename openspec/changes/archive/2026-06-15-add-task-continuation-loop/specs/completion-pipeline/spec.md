## MODIFIED Requirements

### Requirement: Five-stage completion pipeline
When a Task reaches Delivered state, the system SHALL execute a six-stage pipeline: Freeze, Review, Merge, Memorialize, Cleanup, Continuation. Each stage MUST complete (or fail gracefully) before the next begins. The Continuation stage evaluates whether the Task's work is complete (Audit Confirmed) or requires another iteration, and if so dispatches a new Task via the ContinuationCoordinator.

#### Scenario: Normal completion flow without continuation
- **WHEN** Task A reaches Delivered state and Audit returns Confirmed
- **THEN** the pipeline executes: Freeze → Review → Merge → Memorialize → Cleanup → Continuation
- **AND** the Continuation stage determines no further iteration is needed
- **AND** Task A ends in Archived state

#### Scenario: Completion flow triggers continuation
- **WHEN** Task A reaches Delivered state and Audit returns NeedsFix
- **THEN** the pipeline executes: Freeze → Review → Merge → Memorialize → Cleanup → Continuation
- **AND** the Continuation stage dispatches a new Task B as iteration 2 of the continuation chain
- **AND** Task A ends in Archived state while Task B begins its own lifecycle

#### Scenario: Pipeline stage failure halts chain
- **WHEN** the Merge stage fails with an unresolvable conflict
- **THEN** the Continuation stage is NOT reached, the user is notified, and no continuation Task is dispatched

### Requirement: Review stage uses risk-based grading (C+D hybrid)
The Review stage SHALL assess risk (based on diff size, file types, task type) and route accordingly: low-risk tasks get automated rule checks (compile/test); medium/high-risk tasks get an Audit Agent dispatched—producing a real `AuditReport` via `AuditAgent::new` (not a hardcoded stub); the Orchestrator makes the final decision based on Audit report. The Audit verdict (Confirmed / FalsePositive / NeedsFix / NeedsHumanReview) SHALL be forwarded to the Continuation stage to drive continuation decisions.

#### Scenario: Low-risk auto-accepted
- **WHEN** Task A's diff is 10 lines in a .md file
- **THEN** rule checks (compile/test) run automatically
- **AND** if checks pass, the verdict is treated as Confirmed for continuation purposes
- **AND** the Task is accepted without dispatching an Audit Agent

#### Scenario: High-risk spawns real Audit Agent
- **WHEN** Task B's diff is 500 lines across 8 .rs files
- **THEN** an Audit Agent is spawned via `AuditAgent::new` with read-only access to Task B's workspace and diff
- **AND** the Audit Agent produces a structured report with verdict, confidence, and findings
- **AND** the verdict is forwarded to the Continuation stage

#### Scenario: Audit verdict forwarded to continuation
- **WHEN** the Audit Agent returns NeedsFix with findings citing failing tests
- **THEN** the Continuation stage receives the full audit report and uses the findings to construct the continuation prompt seed

### Requirement: Dependency chain auto-activates successor tasks
If a Task was dispatched with a dependency (e.g., "add 2FA after auth refactor"), when the predecessor completes and merges, the successor SHALL be automatically activated with a fresh workspace based on the updated main branch. The activation SHALL dispatch the successor Task via `dispatch_task` (not merely log it), creating a real Task with its own lifecycle.

#### Scenario: Successor activated and dispatched after merge
- **WHEN** Task A (refactor auth) completes and merges successfully
- **THEN** `DependencyGraph::drain_successors` returns Task B (add 2FA, which was waiting on A)
- **AND** Task B is dispatched via `dispatch_task` with a fresh workspace from the updated main branch
- **AND** Task B enters its own full lifecycle (Sleeping → Waking → ...)

#### Scenario: Successor dispatch produces real Task
- **WHEN** the pipeline drains successors after Task A's merge
- **THEN** each successor receives a new TaskId, is spawned via tokio, and appears in the Task Registry
- **AND** the user can query the successor's progress like any other Task

## ADDED Requirements

### Requirement: CompletionOutcome includes Continue variant
The `CompletionOutcome` enum SHALL include a `Continue` variant carrying the continuation reason, a prompt seed for the next iteration, the chain ID, and the iteration number. All code that matches on `CompletionOutcome` MUST handle the `Continue` variant.

#### Scenario: Pipeline produces Continue outcome
- **WHEN** the Review stage returns NeedsFix and the ContinuationCoordinator decides to continue
- **THEN** the pipeline returns `CompletionOutcome::Continue { reason: NeedsFix { findings }, next_prompt_seed, chain_id, iteration: 2 }`

#### Scenario: Continue outcome consumed by coordinator
- **WHEN** the Continuation stage receives `CompletionOutcome::Continue`
- **THEN** the ContinuationCoordinator uses the `next_prompt_seed` and `chain_id` to dispatch the next iteration Task

### Requirement: Completion pipeline runs in production CLI path
The production CLI (`real.rs`) SHALL invoke `CompletionPipeline::run` when a Task reaches `Delivered` state. The pipeline SHALL run in an isolated tokio task to avoid blocking the heartbeat poll loop. The pipeline's `CompletionOutcome` SHALL be communicated back via a channel.

#### Scenario: Task Delivered triggers pipeline in production
- **WHEN** the production CLI's poll loop detects Task A transitioning to Delivered
- **THEN** `CompletionPipeline::run(task_a_id)` is invoked in a spawned tokio task
- **AND** the poll loop continues processing other Task heartbeats without waiting

#### Scenario: Pipeline outcome feeds back to orchestrator
- **WHEN** the pipeline completes with `Merged` or `Continue`
- **THEN** the outcome is sent via a channel to the Orchestrator, which updates the Task Registry and (if Continue) triggers continuation dispatch
