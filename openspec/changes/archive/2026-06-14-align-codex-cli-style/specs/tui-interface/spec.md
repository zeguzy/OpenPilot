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
