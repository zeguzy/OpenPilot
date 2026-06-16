## ADDED Requirements

### Requirement: Orchestrator drains subtask requests and spawns children
The Orchestrator SHALL drain the `subtask_request_queue` on each poll loop iteration. For each `SubTaskRequest`, the Orchestrator SHALL call `dispatch_task(description, focus, [], Some(parent_id))` to create a child Task. The child's `SubTaskNotificationQueue` SHALL be stored in a `subtask_notifications` map keyed by parent_task_id.

#### Scenario: Subtask request drained and child spawned
- **WHEN** the Orchestrator's poll loop finds a `SubTaskRequest { description: "do X", parent_id: "task-3", ... }`
- **THEN** `dispatch_task("do X", [], [], Some("task-3"))` is called
- **AND** the resulting child Task ID is stored in the Task Registry with `parent_task_id = Some("task-3")`
- **AND** a `SubTaskNotificationQueue` is created and associated with `"task-3"`

### Requirement: Orchestrator detects child completion and notifies parent
The Orchestrator SHALL check for child Tasks that have reached a terminal state (Delivered/Stuck/Error) whose parent is in Waiting. For each such child, the Orchestrator SHALL construct a notification message and inject it into the parent's steering channel.

#### Scenario: Child delivered, parent woken
- **WHEN** child Task "task-5" (parent="task-3") reaches Delivered
- **AND** parent "task-3" is in Waiting
- **THEN** Orchestrator injects `Message::user("[Sub-task result] ...")` into task-3's steering channel
- **AND** task-3 transitions back to OnIt on its next steering poll

#### Scenario: Child stuck, parent notified
- **WHEN** child Task "task-5" (parent="task-3") reaches Stuck with reason "compile error"
- **AND** parent "task-3" is in Waiting
- **THEN** Orchestrator injects `Message::user("[Sub-task error] compile error")` into task-3's steering channel
