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
