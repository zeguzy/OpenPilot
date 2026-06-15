## ADDED Requirements

### Requirement: stream_foreground with buffer and dispatch detection
The stream_foreground method SHALL buffer the first line of LLM output. After stream completion, if the full text starts with `OPCA_DISPATCH:`, extract the description and send StreamEvent::Dispatch. Otherwise send StreamEvent::Done.

#### Scenario: First line buffered then flushed
- **WHEN** LLM streams a non-dispatch response
- **THEN** the first line is held in line_buffer until newline or Done, then flushed as Delta

### Requirement: handle_message no longer calls LLM
handle_message SHALL return Reply::Foreground(empty) for all non-task-status messages. The actual LLM call is handled by stream_foreground asynchronously.

#### Scenario: handle_message returns immediately
- **WHEN** handle_message is called with a user message
- **THEN** it returns within microseconds without blocking on LLM

### Requirement: dispatch error visible
If dispatch fails (e.g., workspace creation error), the error message SHALL be displayed in the TUI as an Error chat item, not silently swallowed.

#### Scenario: Workspace creation failure
- **WHEN** dispatch fails with "not a git repository"
- **THEN** the TUI shows "dispatch-error: not a git repository"
