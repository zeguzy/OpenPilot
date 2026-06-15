## ADDED Requirements

### Requirement: Continuation chain lifecycle
The system SHALL support continuation chains—sequences of Tasks where each completed Task may trigger a new Task to continue unfinished work. Each chain has a unique `ChainId`, a root Task, a current active Task, and a status of `Active` or `Terminated`. A chain MUST NOT modify the state machine of any individual Task; each continuation iteration is a fresh Task with its own lifecycle.

#### Scenario: Chain created on first continuation
- **WHEN** Task A completes with `CompletionOutcome::Continue` for the first time
- **THEN** a new `ContinuationChain` is created with a fresh `ChainId`, `root_task_id = A`, `current_task_id` set to the newly dispatched Task B, and `status = Active`

#### Scenario: Chain tracks iterations
- **WHEN** Task B (iteration 2 of chain X) completes with `Continue`
- **THEN** chain X's `current_task_id` updates to the newly dispatched Task C, and an `IterationRecord` for Task B is appended to chain X's history

#### Scenario: Chain terminates on confirmed completion
- **WHEN** Task D (iteration 4 of chain X) completes and Audit returns `Confirmed`
- **THEN** chain X's status becomes `Terminated(ConfirmedComplete)` and no further Task is dispatched

#### Scenario: Each iteration is a new Task with independent lifecycle
- **WHEN** a continuation chain dispatches iteration 3
- **THEN** the dispatched Task has a new `TaskId` distinct from all prior iterations, a new workspace created from the current main branch, and progresses through the full `Sleeping → ... → Delivered` lifecycle independently

### Requirement: Continuation decision requires audit verification (two-layer completion)
A continuation chain SHALL NOT terminate based on a Task's self-reported completion alone. The chain terminates only when the Audit agent returns `Confirmed`. If Audit returns `FalsePositive` or `NeedsFix`, the chain SHALL dispatch a continuation Task carrying the audit findings as feedback. If Audit returns `NeedsHumanReview`, the chain SHALL terminate with that reason and notify the user.

#### Scenario: Self-reported done without audit confirmation continues chain
- **WHEN** Task A (iteration 1) self-reports complete and Audit returns `FalsePositive`
- **THEN** the chain dispatches Task B (iteration 2) with a prompt seed containing the audit findings explaining why the completion claim was false

#### Scenario: Audit confirmed terminates chain
- **WHEN** Task C (iteration 3) completes and Audit returns `Confirmed`
- **THEN** the chain terminates with `ConfirmedComplete` and no further Task is dispatched

#### Scenario: NeedsFix triggers targeted continuation
- **WHEN** Audit returns `NeedsFix` with findings citing a specific failing test
- **THEN** the continuation prompt seed includes the exact test name, failure output, and instruction to fix it

#### Scenario: NeedsHumanReview stops chain
- **WHEN** Audit returns `NeedsHumanReview` with confidence below the configured threshold
- **THEN** the chain terminates with `NeedsHumanReview` reason, the user is notified, and no further Task is dispatched automatically

### Requirement: Continuation budget enforces multi-dimensional limits
Each continuation chain SHALL be bound by a `ContinuationBudget` with four dimensions: maximum iterations, maximum total cost (USD), maximum total duration, and maximum consecutive no-progress rounds. If ANY dimension is exhausted, the chain SHALL terminate immediately with `BudgetExhausted` specifying the exhausted dimension, and no further Task SHALL be dispatched.

#### Scenario: Iteration limit reached
- **WHEN** chain X is configured with `max_iterations = 10` and iteration 10 completes with `Continue`
- **THEN** the chain terminates with `BudgetExhausted(Iterations)` and the user is notified with the completed iteration count

#### Scenario: Cost limit reached
- **WHEN** chain X has `max_total_cost_usd = 5.0` and the accumulated cost after iteration 4 reaches 5.2 USD
- **THEN** the chain terminates with `BudgetExhausted(Cost)` before dispatching iteration 5

#### Scenario: Duration limit reached
- **WHEN** chain X has `max_total_duration = 30 minutes` and the total wall-clock time exceeds 30 minutes
- **THEN** the chain terminates with `BudgetExhausted(Duration)` even if the current Task is still running

#### Scenario: Budget is configurable per chain
- **WHEN** the user dispatches with `/continue --max-iterations 20 --budget 10.0`
- **THEN** the chain's budget is set to `max_iterations = 20` and `max_total_cost_usd = 10.0`, overriding config defaults

### Requirement: No-progress detection prevents doom loops
The system SHALL detect when consecutive continuation iterations produce no meaningful progress and terminate the chain. Progress is measured by diff significance—non-empty diff with file set changes distinct from the prior iteration. Consecutive rounds with empty diffs or substantively identical diffs SHALL increment a no-progress counter; when it reaches the configured threshold (default 2), the chain SHALL terminate with `NoProgress`.

#### Scenario: Empty diff triggers no-progress
- **WHEN** iteration 3 produces an empty diff (no files changed) and iteration 4 also produces an empty diff
- **THEN** the no-progress counter reaches 2 and the chain terminates with `NoProgress`

#### Scenario: Substantively identical diff triggers no-progress
- **WHEN** iteration 5's diff modifies the same lines in the same files as iteration 4 with no net change
- **THEN** the no-progress counter increments and the chain may terminate if the threshold is reached

#### Scenario: Meaningful progress resets counter
- **WHEN** iteration 2 has no progress but iteration 3 modifies new files not touched in iteration 2
- **THEN** the no-progress counter resets to 0

#### Scenario: Repeated same-finding escalation
- **WHEN** 3 consecutive iterations all receive Audit findings citing the same file and same problem category
- **THEN** the chain terminates with `NoProgress` and escalates to `NeedsHumanReview` notification

### Requirement: Continuation dispatches new Task with parent linkage
When a continuation chain dispatches a new iteration, it SHALL call `dispatch_task` with a `parent_task_id` linking the new Task to the prior iteration. The new Task's workspace SHALL be created from the current main branch (not the parent's workspace). The new Task's prompt SHALL include a structured continuation seed containing: the prior iteration's audit findings, failing tests (if any), and a summary of work completed so far retrieved from Cold Store.

#### Scenario: Continuation Task has parent linkage
- **WHEN** chain X dispatches iteration 2 (Task B) after iteration 1 (Task A)
- **THEN** Task B's `parent_task_id = Task A's id` and this linkage is queryable via the chain registry

#### Scenario: Continuation workspace based on main branch
- **WHEN** Task B (iteration 2) is dispatched and Task A (iteration 1) already merged changes to main
- **THEN** Task B's workspace is created from the updated main branch including Task A's merged changes

#### Scenario: Continuation prompt includes prior feedback
- **WHEN** chain X dispatches iteration 3 after iterations 1 and 2 both failed Audit
- **THEN** the prompt seed for iteration 3 includes a summary of both prior failures, their audit findings, and Cold Store references for detailed context retrieval

### Requirement: Continuation policy is pluggable
The decision of whether to continue a chain SHALL be delegated to a `ContinuationPolicy` trait with a default implementation. Plugins SHALL be able to replace the default policy to customize continuation behavior (e.g., different thresholds, custom termination logic, integration with external CI systems).

#### Scenario: Default policy continues on NeedsFix
- **WHEN** the default policy receives Audit `NeedsFix` and the budget allows another iteration
- **THEN** it returns `Continue(NeedsFix)` and the coordinator dispatches a new Task

#### Scenario: Custom policy from plugin
- **WHEN** a plugin registers a custom `ContinuationPolicy` that always terminates after 3 iterations regardless of audit verdict
- **THEN** the coordinator uses the custom policy's decision and terminates the chain after iteration 3

### Requirement: Chain termination reason classification
When a continuation chain terminates, the system SHALL record a `ChainTerminationReason` that classifies the termination cause. The reason SHALL drive the user notification format and any post-termination cleanup behavior.

#### Scenario: Confirmed completion notification
- **WHEN** a chain terminates with `ConfirmedComplete`
- **THEN** the user receives a concise notification: "Continuation chain completed: N iterations, $X cost"

#### Scenario: Budget exhausted notification
- **WHEN** a chain terminates with `BudgetExhausted(Cost)`
- **THEN** the user receives a notification highlighting the cost limit, completed iterations, and the last audit verdict for diagnosis

#### Scenario: User cancellation
- **WHEN** the user runs `/stop-continuation <chain-id>`
- **THEN** the chain terminates with `UserCancelled`, the currently running Task (if any) is allowed to complete normally but no further iteration is dispatched

### Requirement: User can start and stop continuation chains manually
The CLI SHALL provide `/continue` to manually start a continuation chain on a completed Task or a new prompt, and `/stop-continuation` to terminate an active chain. These commands give the user explicit control over autonomous continuation.

#### Scenario: Manual chain start
- **WHEN** the user runs `/continue "fix all failing tests in the auth module"`
- **THEN** a new continuation chain is created, the first Task is dispatched with the prompt, and the chain ID is displayed to the user

#### Scenario: Manual chain stop
- **WHEN** the user runs `/stop-continuation chain-abc123`
- **THEN** chain `chain-abc123` is marked `Terminated(UserCancelled)` and no further iteration is dispatched; any currently running Task completes normally

#### Scenario: Chain status query
- **WHEN** the user runs `/continue status chain-abc123`
- **THEN** the system displays: chain status, current iteration, budget consumption (iterations used / limit, cost used / limit), and a summary of each completed iteration

### Requirement: Continuation chain observability via heartbeats
Each continuation iteration SHALL push heartbeats through the standard Task heartbeat mechanism. The Orchestrator's Active Memory SHALL include a summary of active continuation chains: chain ID, current iteration, budget consumption, and last audit verdict.

#### Scenario: Chain progress visible in task registry
- **WHEN** chain X is on iteration 3 and the user asks "how is the continuation going?"
- **THEN** the Orchestrator responds using the chain's current iteration, budget consumption, and last audit verdict from the chain registry

#### Scenario: Chain completion updates registry
- **WHEN** chain X terminates with `ConfirmedComplete`
- **THEN** the chain is removed from the active chains registry and its final summary enters the Orchestrator's Active Memory
