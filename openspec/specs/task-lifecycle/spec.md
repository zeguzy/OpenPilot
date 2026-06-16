## ADDED Requirements

### Requirement: Task follows personified lifecycle state machine
Each Task SHALL progress through a state machine with the following states: Sleeping, Waking, Pondering, OnIt, Waiting, Reviewing, Delivered, Stuck, Axed, Archived. Only valid transitions are allowed; invalid transitions MUST be rejected.

#### Scenario: Normal lifecycle progression
- **WHEN** a Task is created and initialized
- **THEN** it transitions Sleeping → Waking → Pondering → OnIt → Delivered → Reviewing → Archived

#### Scenario: Invalid transition rejected
- **WHEN** a Task in state Sleeping receives a transition to Delivered
- **THEN** the transition is rejected and an error is returned

#### Scenario: Task gets stuck
- **WHEN** a Task in OnIt encounters an unrecoverable error
- **THEN** it transitions to Stuck and a heartbeat is pushed to Orchestrator

#### Scenario: Task cancelled from any state
- **WHEN** Orchestrator or user cancels a Task in OnIt
- **THEN** the Task transitions to Axed, then to Archived after cleanup

### Requirement: State transitions push heartbeat automatically
Whenever a Task's state changes, a heartbeat (Layer 1 context) SHALL be automatically pushed to the Orchestrator. The heartbeat contains: state, progress percentage, and a one-line summary of current activity.

#### Scenario: Heartbeat on state change
- **WHEN** Task A transitions from Pondering to OnIt with 0% progress
- **THEN** a heartbeat `{"state": "on-it", "progress": 0, "summary": "starting execution"}` is pushed to Orchestrator

### Requirement: Task panic converts to Crashed state
If a Task's tokio task panics, the Task state SHALL be set to a terminal error state and the Orchestrator and user SHALL be notified. The Task's Memory and workspace state MUST remain recoverable.

#### Scenario: Task panic during execution
- **WHEN** Task B's tokio task panics during OnIt
- **THEN** Task B's state becomes a terminal error state
- **AND** Orchestrator receives a notification with the panic message
- **AND** Task B's workspace and Memory remain intact for inspection

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
## ADDED Requirements

### Requirement: Task output channel for real-time streaming
The Task struct SHALL include an `output_tx: UnboundedSender<TaskOutput>` channel. The agent loop SHALL push events to this channel as they occur: LLM text deltas, tool call starts/ends, highlights, heartbeat transitions.

#### Scenario: Text delta pushed to output
- **WHEN** the Task's agent loop receives a ProviderEvent::TextDelta("hello")
- **THEN** a `TaskOutput::TextDelta("hello")` is pushed to output_tx

#### Scenario: Tool call pushed to output
- **WHEN** the Task executes a tool call (read, write, bash)
- **THEN** a `TaskOutput::ToolCall { name, args }` is pushed to output_tx

#### Scenario: Highlight pushed to output
- **WHEN** the Task calls report_highlight
- **THEN** a `TaskOutput::Highlight(highlight)` is pushed to output_tx

### Requirement: TaskOutput enum types
```rust
pub enum TaskOutput {
    TextDelta(String),
    ToolCall { name: String, args: String },
    ToolResult { name: String, success: bool, summary: String },
    Highlight(Highlight),
    StatusChanged { status: TaskStatus, progress: f64, summary: String },
    Done,
}
```

#### Scenario: All output types are covered
- **WHEN** a Task runs through a complete lifecycle
- **THEN** output_tx receives TextDelta, ToolCall, ToolResult, Highlight, StatusChanged, and Done events

### Requirement: Task registers file and shell tools
Task::new() SHALL register the following tools into the ToolRegistry: `read`, `write`, `edit`, `bash`, `grep`, `find`, `ls`, `todo_write`, `report_highlight`, `request_clarification`. The `dispatch_subtask` tool SHALL be registered when the `sub-agents` feature is enabled.

#### Scenario: Task has file manipulation tools
- **WHEN** a Task is created via Task::new()
- **THEN** the ToolRegistry contains read, write, edit, bash, grep, find, and ls tools
- **AND** the LLM can call these tools to explore and modify the workspace

#### Scenario: Task has communication tools
- **WHEN** a Task is created
- **THEN** the ToolRegistry contains todo_write, report_highlight, and request_clarification
- **AND** the LLM can track progress, report findings, and ask for clarification

### Requirement: Task Delivered heartbeat carries final output summary
When a Task transitions to Delivered, the heartbeat summary SHALL contain the first 200 characters of the final assistant message text, not a hardcoded "delivered" string.

#### Scenario: Delivered heartbeat shows real summary
- **WHEN** Task A completes with final message "I refactored the auth module into OAuth2..."
- **THEN** the Delivered heartbeat summary starts with "I refactored the auth module..."
- **AND** the Orchestrator and TUI can display what the Task actually did

### Requirement: Task system prompt includes project context
When a Task is dispatched, the Orchestrator SHALL load AGENTS.md from the project path and inject it into the Task's system prompt under a `## Project Context` section.

#### Scenario: AGENTS.md injected into system prompt
- **WHEN** a Task is dispatched in a project with AGENTS.md
- **THEN** the Task's system prompt contains a `## Project Context` section
- **AND** the section includes the AGENTS.md content (with @import expansion)

#### Scenario: No AGENTS.md
- **WHEN** a Task is dispatched in a project without AGENTS.md
- **THEN** the system prompt does not include a Project Context section
