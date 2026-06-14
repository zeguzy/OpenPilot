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
