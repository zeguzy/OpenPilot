## ADDED Requirements

### Requirement: Continuation check is subtask-aware
The `check_continuations` logic SHALL skip parent Tasks that are in Waiting for subtasks. A parent in Waiting is not "stuck" — it is intentionally waiting for child completion. The continuation coordinator SHALL NOT evaluate or terminate a chain whose current task is in Waiting with pending subtasks.

#### Scenario: Waiting parent not evaluated for continuation
- **WHEN** parent "task-3" is in a continuation chain AND is in Waiting for 1 subtask
- **THEN** `check_continuations` skips "task-3" — no evaluate_continuation call
- **AND** the chain remains Active

#### Scenario: Parent evaluated after subtask completes
- **WHEN** parent "task-3" transitions from Waiting back to OnIt (subtask completed)
- **AND** later reaches Delivered
- **THEN** `check_continuations` evaluates continuation as normal
