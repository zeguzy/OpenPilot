## ADDED Requirements

### Requirement: Task delegation via dispatch_subtask tool
A Task SHALL have access to a `dispatch_subtask` tool that spawns a child Task with a scoped prompt. The child Task inherits a subset of the parent's FocusContract dimensions, runs in the parent's workspace (not a fresh one), and reports back via the standard heartbeat/highlight mechanism plus a final `subtask_result` tool call. The parent Task enters `Waiting` state while sub-tasks run.

#### Scenario: Parent dispatches a sub-task
- **WHEN** Task A calls `dispatch_subtask(prompt="Find all uses of deprecated_api()", focus_subset=["diff-sanity"])`
- **THEN** a new Task B is created with `parent_task_id = Task A`
- **AND** Task B uses Task A's workspace (not a fresh workspace from main)
- **AND** Task B's FocusContract has dimensions `["diff-sanity"]` (subset of A's)
- **AND** Task A enters `Waiting` state

#### Scenario: Sub-task reports back
- **WHEN** Task B (sub-task of A) reaches `Delivered`
- **THEN** Task A is notified with Task B's summary and any highlights
- **AND** Task A transitions out of `Waiting` to `OnIt` to incorporate the result

#### Scenario: Sub-task failure surfaces to parent
- **WHEN** Task B transitions to `Stuck` after 3 failed attempts
- **THEN** Task A is notified with the failure context
- **AND** Task A may choose to retry with a different decomposition or self-handle

### Requirement: Sub-task workspace scoping
Sub-Tasks SHALL operate in the parent Task's workspace by default. This is critical for performance: spawning a fresh workspace per sub-task would dwarf the delegation cost. If a sub-task genuinely needs isolation (e.g., destructive operations), the parent can pass `scope = Isolated` to get a fresh worktree.

#### Scenario: Default shared workspace
- **WHEN** Task A dispatches sub-task B without specifying scope
- **THEN** Task B operates on Task A's workspace path
- **AND** changes Task B makes are visible to Task A after the sub-task completes

#### Scenario: Explicit isolation
- **WHEN** Task A dispatches sub-task B with `scope = Isolated`
- **THEN** Task B gets a fresh workspace from main branch
- **AND** Task B's changes are merged back to Task A's workspace via the standard merge flow on completion

### Requirement: Delegation depth limit
The system SHALL enforce a maximum delegation depth of 2 by default. Root Task is depth 0; sub-tasks spawned from root are depth 1; sub-sub-tasks are depth 2; depth 3 is forbidden. This prevents runaway spawning where each sub-task spawns more sub-tasks indefinitely.

#### Scenario: Depth 2 allowed
- **WHEN** Task A (depth 0) dispatches sub-task B (depth 1) which dispatches sub-task C (depth 2)
- **THEN** all three tasks run normally

#### Scenario: Depth 3 rejected
- **WHEN** Task C (depth 2) attempts to dispatch sub-task D
- **THEN** the `dispatch_subtask` tool returns an error: "Maximum delegation depth (2) reached"
- **AND** no Task D is created

#### Scenario: Depth configurable
- **WHEN** the user configures `.agent/config.toml` with `[sub_agent] max_depth = 3`
- **THEN** the depth limit becomes 3 for all Tasks in this session

### Requirement: Parallel sub-task limit
A parent Task SHALL be limited to 3 concurrent sub-tasks by default. Spawning a 4th concurrent sub-task SHALL return an error. As sub-tasks complete, the parent may dispatch new ones. This prevents resource exhaustion from runaway parallelism.

#### Scenario: Parallel limit enforced
- **WHEN** Task A has 3 concurrent sub-tasks (B, C, D) running and tries to dispatch a 4th (E)
- **THEN** the `dispatch_subtask` tool returns an error: "Maximum concurrent sub-tasks (3) reached; wait for one to complete"

#### Scenario: Slot freed on completion
- **WHEN** sub-task B completes and Task A attempts to dispatch sub-task E
- **THEN** the dispatch succeeds because only 2 sub-tasks (C, D) are now running

#### Scenario: Parallel limit configurable
- **WHEN** the user configures `[sub_agent] max_parallel_per_parent = 5`
- **THEN** Task A may have up to 5 concurrent sub-tasks

### Requirement: Sub-task heartbeat aggregation
Sub-task heartbeats SHALL be aggregated into the parent Task's heartbeat for unified observability. The Orchestrator's view of Task A includes a "Sub-tasks" section showing each child's status. Layer 1 heartbeats from sub-tasks are folded into the parent's Layer 1 (not double-counted); Layer 2 highlights from sub-tasks bubble up to the parent's Layer 2 only if they meet escalation criteria (severity major or higher).

#### Scenario: Sub-task status in parent heartbeat
- **WHEN** Task A is `Waiting` with sub-tasks B (status `OnIt`, 50% progress) and C (status `OnIt`, 20% progress)
- **THEN** Task A's Layer 1 heartbeat includes: `subtasks: [{id: B, status: OnIt, progress: 0.5}, {id: C, status: OnIt, progress: 0.2}]`

#### Scenario: Major highlight escalates
- **WHEN** sub-task B emits a highlight with severity `critical`
- **THEN** the highlight is forwarded to Task A's Layer 2 stream with prefix `[subtask B]`

#### Scenario: Minor highlight does not escalate
- **WHEN** sub-task B emits a highlight with severity `info`
- **THEN** the highlight is recorded in B's history but NOT forwarded to Task A's Layer 2

### Requirement: Sub-task result consumed by parent
On sub-task completion, the parent Task SHALL receive a structured result containing: sub-task ID, summary text, list of artifacts produced (file paths), list of findings (if any). The parent Task's prompt context is extended with this result so subsequent reasoning can reference it. The parent's TodoWrite list is automatically updated to mark the delegated item as completed.

#### Scenario: Result includes artifacts
- **WHEN** sub-task B completes after refactoring `src/auth.rs`
- **THEN** the result delivered to Task A includes `artifacts: ["src/auth.rs"]` and `summary: "Refactored auth to use new token format"`

#### Scenario: Parent TodoWrite auto-updates
- **WHEN** Task A's todo list contains "Find all uses of deprecated_api()" marked `in_progress`
- **AND** sub-task B completes that work
- **THEN** the todo item is automatically marked `completed` in Task A's list

### Requirement: Sub-task lifecycle is abbreviated
Sub-tasks SHALL skip the Phase 0 Intent Gate and Phase 1 Codebase Assessment phases — the parent has already done these. Sub-tasks start at Phase 2 (Execution) directly. Sub-tasks still enforce the Evidence Gate (Phase 3) before `Delivered`.

#### Scenario: Sub-task skips Phase 0
- **WHEN** sub-task B is spawned
- **THEN** B's first turn does NOT ask clarifying questions; B starts executing the parent's prompt immediately

#### Scenario: Sub-task enforces Evidence Gate
- **WHEN** sub-task B has finished its work
- **THEN** B runs the project's evidence commands (build/test/clippy) before transitioning to `Delivered`
- **AND** if verification fails, B enters the 3-strike loop like any Task
