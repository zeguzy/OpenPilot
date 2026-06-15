## ADDED Requirements

### Requirement: CLI input never blocks
The CLI frontend SHALL maintain a constantly-readable input loop. While background Tasks are running, the user SHALL be able to type and submit new messages immediately without waiting.

#### Scenario: Input while task runs
- **WHEN** Task A is running in the background
- **THEN** the user can type and submit a new message
- **AND** the new message is routed to Orchestrator without delay

### Requirement: Silent background mode by default
Background Tasks SHALL NOT produce output to the terminal by default. The user is informed of Task completion via notifications and can query progress at any time.

#### Scenario: Background task produces no output
- **WHEN** Task A is running and generating logs internally
- **THEN** no logs or intermediate output appear in the terminal
- **AND** the terminal remains available for user input

#### Scenario: Completion notification
- **WHEN** Task A completes
- **THEN** a notification line appears: `🔔 Ag: A done, modified 5 files`
- **AND** the notification does not interrupt user input

### Requirement: User can query task progress
The user SHALL be able to ask the Orchestrator about any Task's progress at any time. The Orchestrator responds using the latest heartbeat.

#### Scenario: Progress query
- **WHEN** user types "how is task A going?"
- **THEN** Orchestrator responds with Task A's latest heartbeat (state, progress, summary)

#### Scenario: List all active tasks
- **WHEN** user types "what's running?"
- **THEN** Orchestrator lists all active Tasks with their states and progress

### Requirement: Pending review indicator
When high-risk Tasks are pending review, the CLI SHALL show a subtle status indicator (e.g., "2 tasks pending review") without interrupting the user.

#### Scenario: Pending review shown
- **WHEN** 2 high-risk Tasks are completed but pending user review
- **THEN** a status line shows "2 tasks pending review" in the prompt area
- **AND** no popup or interruption occurs

### Requirement: User can accept or reject completed tasks
The user SHALL be able to accept (merge) or reject (discard/return) completed Tasks via simple commands.

#### Scenario: Accept task
- **WHEN** user types "accept task A"
- **THEN** Task A's changes are merged into the main project

#### Scenario: Reject task with feedback
- **WHEN** user types "reject task A, fix the token validation"
- **THEN** Task A is returned to OnIt with the feedback as a steering message
## ADDED Requirements

### Requirement: CLI auto-selects provider from config and environment
The CLI SHALL construct a real LLM Provider by resolving the provider kind through: `--provider` CLI flag > `[provider] kind` in config.toml > model-name-prefix inference (claude→anthropic, glm→zhipu, gpt→openai) > default "anthropic". The API key SHALL be read from the preset's environment variable.

#### Scenario: Provider from config.toml
- **WHEN** `.agent/config.toml` has `[provider] kind = "zhipu"` and `ZHIPU_API_KEY` env is set
- **THEN** the CLI constructs an `OpenAIProvider` with the Zhipu base URL

#### Scenario: Missing API key gives clear error
- **WHEN** provider kind resolves to "zhipu" but `ZHIPU_API_KEY` is not set
- **THEN** the CLI prints an error listing available env vars and exits

### Requirement: Model resolved from config.toml
The CLI SHALL resolve the model id through: `--model` flag > `OPCA_MODEL` env > `[model] default` in config.toml > built-in default.

#### Scenario: Model from config.toml
- **WHEN** no `--model` flag and no `OPCA_MODEL` env, but config.toml has `[model] default = "glm-4.5"`
- **THEN** the model id is `"glm-4.5"`

### Requirement: Graceful shutdown on /quit, Ctrl+C, Ctrl+D
The CLI SHALL shut down cleanly when the user types `/quit`, presses Ctrl+C, or presses Ctrl+D. All background threads (reedline input loop, notification poll) SHALL be aborted.

#### Scenario: /quit exits cleanly
- **WHEN** user types `/quit` in the REPL
- **THEN** the main loop breaks, `shutdown()` aborts the input thread, and the process exits with code 0

#### Scenario: Ctrl+C exits cleanly
- **WHEN** user presses Ctrl+C in the REPL
- **THEN** the reedline input loop breaks immediately and the process exits

### Requirement: --provider CLI flag overrides config
The CLI SHALL accept `--provider <KIND>` to override the provider kind from config.toml.

#### Scenario: Explicit provider flag
- **WHEN** user runs `opca --provider deepseek --model deepseek-chat`
- **THEN** the provider kind is "deepseek" regardless of config.toml settings
## MODIFIED Requirements

### Requirement: CLI auto-selects provider from config and environment
The CLI SHALL construct a real LLM Provider by resolving the provider kind through: `--provider` CLI flag > `[provider] kind` in config.toml > model-name-prefix inference > default "anthropic". The API key SHALL be read from the preset's environment variable. The TUI launches after provider construction.

#### Scenario: TUI launches with real provider
- **WHEN** `.agent/config.toml` has `[provider] kind = "zhipu"` and `ZHIPU_API_KEY` env is set
- **THEN** the TUI launches with a Zhipu-backed provider and status bar shows "glm-4.5"

## ADDED Requirements

### Requirement: /model command switches model at runtime
`/model <name>` SHALL switch the active model without restarting. The next LLM call uses the new model.

#### Scenario: Switch to cheaper model
- **WHEN** user types `/model glm-4-flash`
- **THEN** the status bar updates to show "glm-4-flash" and subsequent calls use that model

### Requirement: /compact command triggers manual compaction
`/compact` SHALL manually trigger context compaction on the Orchestrator's active memory.

#### Scenario: Manual compact
- **WHEN** user types `/compact`
- **THEN** older messages are compressed into a summary and the chat shows "[compacted N messages]"

### Requirement: /clear command resets conversation
`/clear` SHALL clear the Orchestrator's active memory and chat history. A fresh session starts.

#### Scenario: Clear conversation
- **WHEN** user types `/clear`
- **THEN** the chat area is emptied and the Orchestrator's active memory is reset

### Requirement: /cost command shows detailed usage
`/cost` SHALL display a summary of token usage and estimated cost for the current session.

#### Scenario: Show cost
- **WHEN** user types `/cost`
- **THEN** a panel shows: prompt tokens, completion tokens, total tokens, estimated cost in USD

### Requirement: Graceful shutdown from TUI
`/quit`, Ctrl+C, and Ctrl+D SHALL all restore the terminal (disable raw mode, show cursor) and exit cleanly.

#### Scenario: Clean exit
- **WHEN** user presses Ctrl+C in the TUI
- **THEN** raw mode is disabled, alternate screen is exited, and the process exits with code 0
## ADDED Requirements

### Requirement: stream_foreground method on OrchestratorApi
The OrchestratorApi trait SHALL include `stream_foreground(message, sender)` which spawns an async task that calls the Provider and pushes StreamEvent::Delta for each TextDelta, StreamEvent::Done on completion, or StreamEvent::Error on failure.

#### Scenario: Real provider streams
- **WHEN** RealOrchestrator.stream_foreground is called
- **THEN** a tokio task is spawned that calls provider.stream() and pushes each TextDelta to the channel

#### Scenario: Mock provider streams word by word
- **WHEN** MockOrchestrator.stream_foreground is called
- **THEN** the echo response is pushed word by word with 30ms delay
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
