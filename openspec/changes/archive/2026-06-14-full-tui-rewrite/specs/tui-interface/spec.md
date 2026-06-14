## ADDED Requirements

### Requirement: Full-screen TUI with ratatui
The CLI SHALL render a full-screen TUI using ratatui + crossterm, replacing the reedline line-based REPL. The screen SHALL be divided into: status bar (top), chat area (scrollable, center), task bar (bottom area when tasks active), input area (bottom).

#### Scenario: TUI launches and takes over screen
- **WHEN** user runs `opca` (non-mock)
- **THEN** the terminal switches to raw mode and renders the TUI layout
- **AND** Ctrl+C or /quit restores the terminal and exits cleanly

### Requirement: Status bar shows model, tokens, cost
The status bar SHALL display: provider/model name, cumulative prompt tokens, cumulative completion tokens, estimated cost.

#### Scenario: Status bar updates after each response
- **WHEN** the Orchestrator receives an LLM response with usage data
- **THEN** the status bar updates token counts and cost estimate

### Requirement: Streaming output with line-level Markdown
LLM responses SHALL stream token-by-token. Each completed line SHALL be rendered as Markdown (headers, bold, inline code, code blocks with syntax highlighting) immediately upon receiving the newline character.

#### Scenario: Code block renders with syntax highlighting
- **WHEN** the LLM streams a fenced code block ```rust ... ```
- **THEN** lines inside the fence are rendered with Rust syntax highlighting
- **AND** the code block has a visual border

#### Scenario: Streaming text appears incrementally
- **WHEN** the LLM streams "Hello\nWorld" as two TextDelta events
- **THEN** "Hello" appears first (rendered), then "World" appears (rendered)

### Requirement: Collapsible Task panels in chat stream
When a Task is dispatched, a collapsible panel SHALL be inserted into the chat stream. Default state: collapsed (one-line summary with status emoji, task_id, progress, current activity). Expanded state: full output stream (LLM text, tool calls, highlights).

#### Scenario: Panel appears on dispatch
- **WHEN** Orchestrator dispatches task-0
- **THEN** a collapsed panel appears in the chat stream showing "🔨 task-0 on-it 0% — starting"

#### Scenario: Panel updates in real-time
- **WHEN** task-0 transitions to 60% progress with "editing auth.rs"
- **THEN** the collapsed panel updates to show "🔨 task-0 on-it 60% — editing auth.rs"

#### Scenario: Panel expands to show output
- **WHEN** user runs `/expand task-0` or presses Enter on the focused panel
- **THEN** the panel expands showing all accumulated Task events (tool calls, highlights, text)

#### Scenario: Panel collapses
- **WHEN** user runs `/collapse task-0` or presses Enter on an expanded panel
- **THEN** the panel returns to one-line summary

### Requirement: Task steering mode
`/task <id>` SHALL switch the TUI to Task mode. In Task mode, user input goes to the Task's steering channel. Task output streams to the chat area. `/back` returns to Orchestrator mode.

#### Scenario: Enter Task mode
- **WHEN** user types `/task task-0`
- **THEN** the status bar shows "task-0 | 🔨 on-it 60%" and input now routes to steering

#### Scenario: Send steering message
- **WHEN** user types "don't touch login.rs" in Task mode
- **THEN** the message is sent as SteeringMessage::Inject to task-0
- **AND** the message appears in the chat as a user steering entry

#### Scenario: Return to Orchestrator
- **WHEN** user types `/back` in Task mode
- **THEN** the TUI returns to Orchestrator mode and input routes to handle_message

### Requirement: Multi-line input with tui-textarea
The input area SHALL support multi-line editing via tui-textarea. Enter submits the message; Shift+Enter inserts a newline.

#### Scenario: Multi-line paste
- **WHEN** user pastes a multi-line code block into the input area
- **THEN** the text appears across multiple lines without triggering submit

### Requirement: Scrollable chat area
The chat area SHALL be scrollable with Page Up/Page Down and arrow keys when content exceeds the viewport.

#### Scenario: Scroll up to history
- **WHEN** user presses Page Up
- **THEN** the chat area scrolls up to show earlier messages
