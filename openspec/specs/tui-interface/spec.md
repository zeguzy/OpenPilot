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
## ADDED Requirements

### Requirement: Codex CLI-style minimal layout
The TUI SHALL NOT use a top status bar. The screen SHALL be divided into two regions only: chat area (scrollable, fills most of the screen) and bottom area (working indicator + input). All content SHALL use a 2-column left gutter for visual alignment.

#### Scenario: No top bar on launch
- **WHEN** the TUI launches
- **THEN** there is no status bar at the top of the screen
- **AND** chat content starts from the first line

### Requirement: Spinner-based working indicator
When the LLM is processing (is_working=true), a working status line SHALL appear above the input area showing: braille spinner animation + mode label ("Thinking" for Orchestrator, "Task N" for Task mode) + elapsed seconds + "(esc to interrupt)" hint.

#### Scenario: Working indicator during LLM call
- **WHEN** the user sends a message and the Orchestrator is querying the LLM
- **THEN** a line appears: "⠋ Thinking (3s - esc to interrupt)" with animated spinner

#### Scenario: Esc interrupts waiting
- **WHEN** the user presses Esc while is_working is true
- **THEN** is_working becomes false and the spinner disappears

### Requirement: User messages use muted color
User messages SHALL use Gray color (not green/bold prefix). AI responses SHALL use default foreground with no prefix. Both SHALL be indented by the left gutter.

#### Scenario: User vs AI visual distinction
- **WHEN** a chat contains a user message and an AI response
- **THEN** the user message appears in Gray text and the AI response in default color
- **AND** neither has a "you:" or "opca:" prefix

### Requirement: Minimal borderless input
The input area SHALL have no border. It SHALL use a Cyan "> " prefix for Orchestrator mode and "task-N> " for Task mode. A cursor SHALL be positioned at the end of the input text.

#### Scenario: Input prefix reflects mode
- **WHEN** in Orchestrator mode, the input prefix is "> "
- **WHEN** in Task mode with task-0, the input prefix is "task-0> "

### Requirement: Model and token info in footer
Model name, cumulative prompt tokens, and completion tokens SHALL be available via a footer info function. (Currently rendered on demand via /cost command; future versions may show in a compact bottom line.)

#### Scenario: Footer info available
- **WHEN** render_footer_info(app) is called
- **THEN** it returns "model_name - up:N down:M - $cost"
## ADDED Requirements

### Requirement: Streaming LLM output
LLM responses SHALL stream token-by-token to the TUI. Each TextDelta from the Provider SHALL be appended to the current StreamingAssistant chat item immediately, visible on the next tick (50ms). When the stream completes (Done), the StreamingAssistant SHALL convert to AssistantText.

#### Scenario: Text appears incrementally
- **WHEN** the LLM streams "Hello World" as TextDelta("Hello ") then TextDelta("World")
- **THEN** "Hello " appears on the first tick, then "World" on the next

#### Scenario: Stream completes
- **WHEN** the Provider stream sends Done
- **THEN** the StreamingAssistant item is replaced by AssistantText with the full text
- **AND** is_working becomes false and spinner stops

### Requirement: No blocking on Enter
After pressing Enter, the user message SHALL appear in the chat immediately, without waiting for the LLM response. The handle_message function SHALL NOT call the LLM synchronously.

#### Scenario: Instant user message display
- **WHEN** user presses Enter with a foreground message
- **THEN** the user message appears in chat within one tick (50ms)
- **AND** a blank StreamingAssistant item is created
- **AND** spinner starts

### Requirement: poll_stream consumes channel on tick
The TUI main loop SHALL call poll_stream on every tick. poll_stream drains the stream_rx channel and appends deltas to the last StreamingAssistant item.

#### Scenario: Deltas accumulated between ticks
- **WHEN** 3 TextDelta events arrive between ticks
- **THEN** all 3 are concatenated and appended to the streaming item on the next tick
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

### Requirement: TUI renders thinking content with distinct style
The TUI SHALL render `ThinkingDelta` stream events as dimmed (dark gray) text with a 💭 prefix, visually distinct from normal assistant text. Thinking content SHALL be preserved when the response transitions to text or dispatch — it SHALL NOT be replaced or removed.

#### Scenario: Thinking displayed during streaming
- **WHEN** the LLM stream emits ThinkingDelta events
- **THEN** the TUI renders them as StreamingThinking with dark gray 💭 prefix
- **AND** they are visually distinct from normal text

#### Scenario: Thinking preserved after text starts
- **WHEN** ThinkingDelta events are followed by TextDelta events
- **THEN** the thinking content is finalized as ThinkingText (not deleted)
- **AND** the text content appears below as a new StreamingAssistant block

#### Scenario: Thinking preserved on dispatch
- **WHEN** ThinkingDelta events are followed by a Dispatch event
- **THEN** the thinking content is finalized as ThinkingText before the dispatch replaces the streaming item

### Requirement: Task panel shows description and clean status indicators
The TaskPanel SHALL display the task description (truncated to 50 chars) in both collapsed and expanded views. Status events SHALL use emoji indicators (🤔 pondering, ⚙️ on-it, ✅ done, 😵 stuck, ⏳ waiting) instead of raw text. The collapsed view SHALL use `▸` (running, yellow) or `✓` (done, green) as the panel icon.

#### Scenario: Collapsed panel shows description and status
- **WHEN** a task is running and its panel is collapsed
- **THEN** the display shows `▸ {id} {description...}` with yellow color
- **AND** no `[/expand]` or `[/collapse]` command hints are shown

#### Scenario: Completed task shows green check
- **WHEN** a task is done and its panel is collapsed
- **THEN** the display shows `✓ {id} {description...}` with green color

#### Scenario: Expanded panel shows emoji status events
- **WHEN** a task panel is expanded
- **THEN** each heartbeat event is rendered with an emoji prefix (🤔, ⚙️, ✅, etc.)
- **AND** the raw `|` prefix is not used
