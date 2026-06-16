## MODIFIED Requirements

### Requirement: Task Waiting state pauses execution
When a Task needs input from Orchestrator or user, it SHALL transition to Waiting. A steering reply transitions it back to OnIt. When the Task has pending subtasks and no new tool calls, it SHALL also transition to Waiting. A subtask completion notification (injected via steering) transitions it back to OnIt.

#### Scenario: Task waits for clarification
- **WHEN** Task C encounters an ambiguity it cannot resolve
- **THEN** it transitions to Waiting and pushes a heartbeat with the question
- **AND** when Orchestrator sends a steering reply, Task C transitions back to OnIt

#### Scenario: Task waits for subtask
- **WHEN** Task A has 1 pending subtask and emits a text response without tool calls
- **THEN** it transitions to Waiting with heartbeat "waiting for 1 subtask(s)"
- **AND** when the subtask completes and its result is injected via steering, Task A transitions back to OnIt
