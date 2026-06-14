## 1. Provider Infrastructure

- [x] 1.1 OpenAIProvider: add base_url field + with_base_url() constructor
- [x] 1.2 Create provider/presets.rs with 10 presets + resolve() + normalize_chat_completions_url()
- [x] 1.3 Create config.rs with TOML parsing for [model] and [provider] sections
- [x] 1.4 Add `toml = "0.8"` to workspace dependencies
- [x] 1.5 Add `pub mod prompts` to provider/mod.rs with orchestrator_prompt() and task_prompt()

## 2. CLI Wiring

- [x] 2.1 Delete StubProvider from main.rs
- [x] 2.2 Add create_provider() function with 4-step resolution (CLI > config > inference > default)
- [x] 2.3 Add guess_kind_from_model() for model-prefix-based provider inference
- [x] 2.4 Add --provider <KIND> CLI flag to Cli struct
- [x] 2.5 Read config.toml model.default into model resolution chain
- [x] 2.6 Wire system prompt into query_llm (Orchestrator foreground)
- [x] 2.7 Wire system prompt into Task::build_system_prompt (identity + focus)

## 3. Graceful Shutdown

- [x] 3.1 Ctrl+C breaks reedline input loop directly (instead of sending empty string)
- [x] 3.2 main.rs calls runtime.shutdown() after repl_handle.await completes
- [x] 3.3 repl_handle.take() to avoid move-after-await compilation error

## 4. Configuration

- [x] 4.1 Write .agent/config.toml with zhipu preset (kind=zhipu, model=glm-4.5)

## 5. Tests

- [x] 5.1 Preset resolve by name, alias, case-insensitive (12 unit tests)
- [x] 5.2 normalize_chat_completions_url with various inputs
- [x] 5.3 Config load with present, missing, and invalid TOML
- [x] 5.4 All existing 600+ tests still pass
- [x] 5.5 0 clippy warnings maintained
