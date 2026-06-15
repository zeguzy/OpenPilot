## ADDED Requirements

### Requirement: LLM-driven routing via OPCA_DISPATCH prefix
Routing SHALL be determined by the LLM, not keyword matching. The Orchestrator system prompt instructs the LLM to begin task responses with `OPCA_DISPATCH: <description>`. The stream_foreground method detects this prefix after stream completion and triggers a background dispatch.

#### Scenario: Task message triggers dispatch
- **WHEN** LLM responds with "OPCA_DISPATCH: Explore project structure\n好的，正在探索..."
- **THEN** stream_foreground sends StreamEvent::Dispatch("Explore project structure")
- **AND** poll_stream calls orchestrator.dispatch() and creates a TaskPanel

#### Scenario: Question message streams normally
- **WHEN** LLM responds with "你好！有什么可以帮你的？" (no prefix)
- **THEN** stream_foreground sends StreamEvent::Done
- **AND** the text is displayed as AssistantText

### Requirement: Dispatch prefix hidden from user
The `OPCA_DISPATCH:` prefix line SHALL NOT be visible to the user. The streaming buffer holds the first line until it can be checked. If it contains the prefix, it is discarded. Otherwise it is flushed to the TUI.

#### Scenario: Prefix line not shown
- **WHEN** LLM streams "OPCA_DISPATCH: Write sorting function\n..."
- **THEN** the first line is buffered and never sent as Delta
- **AND** subsequent lines (friendly reply) are streamed normally

### Requirement: Message queue during LLM processing
When is_working is true, Enter SHALL enqueue the message into pending_messages instead of sending immediately. The queue is processed automatically when is_working becomes false. The TUI SHALL display `N queued - next: preview` above the working status.

#### Scenario: Queue message during processing
- **WHEN** user presses Enter while is_working is true
- **THEN** the message is added to pending_messages
- **AND** the input is cleared

#### Scenario: Auto-send queued message
- **WHEN** is_working becomes false and pending_messages is non-empty
- **THEN** the first queued message is automatically sent via send_message

### Requirement: Single dispatch path
Dispatch SHALL only be triggered by poll_stream's StreamEvent::Dispatch handler calling orchestrator.dispatch(). stream_foreground SHALL NOT independently spawn dispatch_task.

#### Scenario: No double dispatch
- **WHEN** LLM output contains OPCA_DISPATCH prefix
- **THEN** exactly one dispatch occurs (via poll_stream)
- **AND** exactly one TaskPanel is created
