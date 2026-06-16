## ADDED Requirements

### Requirement: Sub-agent dispatch registers child in Orchestrator
When a parent Task calls `dispatch_subtask`, the request SHALL be enqueued for the Orchestrator (not executed inline). The Orchestrator SHALL drain the queue, create a child Task via `dispatch_task` with `parent_task_id` set, register it in the Task Registry, and create a `SubTaskNotificationQueue` associated with the parent. The child Task SHALL have its own lifecycle, heartbeat, and Evidence Gate — independent of the parent.

#### Scenario: Child registered and visible
- **WHEN** parent Task calls `dispatch_subtask("analyze auth module")`
- **THEN** the Orchestrator creates a child Task with `parent_task_id = Some(parent_id)`
- **AND** the child appears in the Task Registry
- **AND** `/subtasks <parent_id>` returns the child with its current status

#### Scenario: Child heartbeat forwarded
- **WHEN** child Task transitions from Pondering to OnIt
- **THEN** the child's heartbeat is pushed to the Orchestrator's heartbeat aggregation channel
- **AND** the Orchestrator's Task Registry reflects the child's current status

### Requirement: Parent receives ticket and continues asynchronously
When `dispatch_subtask` tool executes, it SHALL return immediately with a ticket ID. The parent Task SHALL NOT block waiting for the child. The parent's run loop SHALL continue executing other tool calls or transition to Waiting if no further work is available.

#### Scenario: Tool returns ticket immediately
- **WHEN** parent calls `dispatch_subtask("do X")`
- **THEN** the tool returns `ToolResult { content: "Sub-task dispatched (ticket: subtask-N). You will be notified when it completes.", is_error: false }`
- **AND** the parent's run loop continues without blocking

#### Scenario: Parent continues with other tools
- **WHEN** parent calls `dispatch_subtask("do X")` and then `read("file.rs")` in the same tool batch
- **THEN** both tools execute, the dispatch returns a ticket, and the read returns file content
- **AND** the parent processes both results in the same turn

### Requirement: Parent enters Waiting when subtasks are pending
When the parent Task has pending subtasks (dispatched but not yet completed) and the parent's current turn produces no new tool calls, the parent SHALL transition to `Waiting` status. The heartbeat SHALL indicate "waiting for N subtask(s)". When all pending subtasks complete, the parent SHALL be woken via steering and transition back to `OnIt`.

#### Scenario: Parent waits for subtask
- **WHEN** parent has 1 pending subtask and emits a text response without tool calls
- **THEN** parent transitions to Waiting
- **AND** heartbeat summary is "waiting for 1 subtask(s)"

#### Scenario: Parent woken when subtask completes
- **WHEN** child Task reaches Delivered while parent is in Waiting
- **THEN** Orchestrator injects the child's result into parent's steering channel
- **AND** parent transitions back to OnIt
- **AND** the child's result appears in parent's active messages as `[Sub-task result] ...`

#### Scenario: Parent woken when subtask fails
- **WHEN** child Task reaches Stuck or Error while parent is in Waiting
- **THEN** Orchestrator injects `[Sub-task error] ...` into parent's steering channel
- **AND** parent transitions back to OnIt to decide next steps

### Requirement: Subtask notification queue bridges child completion to parent
The Orchestrator SHALL maintain a `SubTaskNotificationQueue` per parent Task. When a child Task reaches a terminal state (Delivered/Stuck/Error), the Orchestrator SHALL construct a `SubTaskNotification` and push it to the parent's queue. The parent's run loop SHALL drain the queue at the start of each turn and inject results as messages.

#### Scenario: Notification pushed on child completion
- **WHEN** child Task "subtask-0" reaches Delivered with summary "auth module analyzed"
- **THEN** a `SubTaskNotification::Completed { sub_task_id, result, verdict: Delivered }` is pushed to the parent's notification queue
- **AND** the parent's steering channel receives a wake-up message

#### Scenario: Parent drains notifications on wake
- **WHEN** parent is woken from Waiting and enters a new turn
- **THEN** all pending notifications in the queue are drained
- **AND** each notification's result is injected as `Message::user("[Sub-task result] ...")` into active messages

### Requirement: Waiting timeout for stuck subtasks
If the parent has been in Waiting for longer than the configured timeout (default 5 minutes), the parent SHALL transition back to OnIt with a timeout message injected. Remaining pending subtasks are abandoned (their results, if they eventually arrive, are ignored).

#### Scenario: Timeout fires
- **WHEN** parent has been Waiting for 5 minutes and child is still running
- **THEN** parent transitions to OnIt
- **AND** `[Sub-task timeout] Subtask did not complete within 5 minutes` is injected into active messages

### Requirement: Parent task ID available in ToolContext
`ToolContext` SHALL include a `task_id: Option<String>` field. When the Orchestrator dispatches a Task, it SHALL set `task_id` in the `ToolContext`. The `DispatchSubtaskTool` SHALL read `ctx.task_id` to populate `SubTaskRequest.parent_id`.

#### Scenario: parent_id populated from context
- **WHEN** Task "task-3" calls `dispatch_subtask`
- **THEN** `SubTaskRequest.parent_id` is `"task-3"` (not empty string)
- **AND** the child Task created from this request has `parent_task_id = Some("task-3")`
