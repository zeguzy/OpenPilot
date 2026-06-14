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
