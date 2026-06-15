## ADDED Requirements

### Requirement: Orchestrator triggers CompletionPipeline on Task Delivered
When the Orchestrator detects a Task transitioning to `Delivered` state (via heartbeat), it SHALL invoke the CompletionPipeline for that Task. This wiring SHALL be active in the production CLI path, not only in tests.

#### Scenario: Heartbeat-driven pipeline trigger
- **WHEN** the Orchestrator's poll loop receives a heartbeat showing Task A transitioned to Delivered
- **THEN** the Orchestrator invokes `CompletionPipeline::run(task_a_id)` in a spawned tokio task
- **AND** the poll loop does not block waiting for the pipeline to complete

#### Scenario: Pipeline outcome updates registry
- **WHEN** the pipeline returns `Merged` for Task A
- **THEN** the Orchestrator updates Task A's registry entry to reflect the merged state
- **AND** if the pipeline returns `Continue`, the Orchestrator forwards it to the ContinuationCoordinator

### Requirement: Orchestrator maintains continuation chains registry
The Orchestrator SHALL maintain a `continuation_chains: HashMap<ChainId, ContinuationChain>` registry tracking all active and recently terminated continuation chains. The registry SHALL be queryable for chain status, budget consumption, and iteration history.

#### Scenario: Active chain registered
- **WHEN** the ContinuationCoordinator creates a new continuation chain for Task A
- **THEN** the chain is registered in `continuation_chains` with its ChainId, root task, and budget

#### Scenario: User queries chain status
- **WHEN** the user asks "what's the status of continuation chain X?"
- **THEN** the Orchestrator responds using the chain registry data: current iteration, budget consumption, last audit verdict

#### Scenario: Terminated chain retained for observability
- **WHEN** a continuation chain terminates (any reason)
- **THEN** the chain remains in the registry with `status = Terminated(reason)` for a configurable retention period (default: until session ends) before removal

### Requirement: dispatch_task supports continuation linkage
The `dispatch_task` method SHALL accept an optional `parent_task_id` parameter. When provided, the newly dispatched Task is linked to the parent via `parent_task_id`, establishing continuation chain lineage. Tasks dispatched without `parent_task_id` are standalone (not part of a chain).

#### Scenario: Continuation Task dispatched with parent linkage
- **WHEN** the ContinuationCoordinator dispatches iteration 2 of chain X
- **THEN** `dispatch_task` is called with `parent_task_id = Some(task_a_id)` where Task A was iteration 1
- **AND** the new Task B's registry entry records `parent_task_id = task_a_id`

#### Scenario: Standalone Task dispatched without parent
- **WHEN** the Orchestrator dispatches a normal background Task from a user request
- **THEN** `dispatch_task` is called with `parent_task_id = None`
- **AND** the Task is not part of any continuation chain

#### Scenario: Chain lineage queryable
- **WHEN** the user asks "what was the parent of Task C?"
- **THEN** the Orchestrator resolves `parent_task_id` from Task C's registry entry and responds with the parent Task's id and summary
