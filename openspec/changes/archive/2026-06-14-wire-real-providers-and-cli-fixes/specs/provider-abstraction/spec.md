## ADDED Requirements

### Requirement: Provider presets for known LLM endpoints
The system SHALL maintain a built-in preset table (`PRESETS`) mapping well-known provider names to their base URLs, environment variable keys, and wire protocols. Presets SHALL cover at least: zhipu, deepseek, ollama, moonshot, openrouter, groq, mistral, anthropic, openai, gemini.

#### Scenario: Resolve provider by canonical name
- **WHEN** `resolve("zhipu")` is called
- **THEN** it returns a preset with `base_url = "https://open.bigmodel.cn/api/paas/v4"` and `env_key = "ZHIPU_API_KEY"`

#### Scenario: Resolve provider by alias
- **WHEN** `resolve("glm")` is called
- **THEN** it returns the zhipu preset (alias match)

#### Scenario: Unknown provider returns None
- **WHEN** `resolve("unknown-provider")` is called
- **THEN** it returns `None`

### Requirement: OpenAIProvider supports custom base URL
The `OpenAIProvider` SHALL accept a custom base URL via `with_base_url(api_key, model, base_url)`, enabling any OpenAI-compatible endpoint (Zhipu, DeepSeek, Ollama, etc.). The original `::new(api_key, model)` SHALL remain backward-compatible with the default OpenAI endpoint.

#### Scenario: Custom base URL routes to Zhipu
- **WHEN** `OpenAIProvider::with_base_url(key, "glm-4-flash", "https://open.bigmodel.cn/api/paas/v4/chat/completions")` streams a message
- **THEN** the HTTP POST targets the Zhipu endpoint, not api.openai.com

### Requirement: URL normalization for chat completions
The system SHALL provide `normalize_chat_completions_url(base)` that appends `/chat/completions` to a bare base URL if not already present.

#### Scenario: Bare base URL gets suffix
- **WHEN** `normalize_chat_completions_url("https://open.bigmodel.cn/api/paas/v4")` is called
- **THEN** the result is `"https://open.bigmodel.cn/api/paas/v4/chat/completions"`

#### Scenario: Already complete URL unchanged
- **WHEN** `normalize_chat_completions_url("https://api.openai.com/v1/chat/completions")` is called
- **THEN** the result equals the input

### Requirement: Config file parsing for model and provider
The system SHALL parse `.agent/config.toml` with `[model] default` and `[provider] kind` + `[provider] base_url` fields. Missing files or invalid TOML SHALL return defaults silently.

#### Scenario: Load config with all fields
- **WHEN** `.agent/config.toml` contains `[model]\ndefault = "glm-4.5"\n[provider]\nkind = "zhipu"`
- **THEN** `Config::load()` returns with `model.default = Some("glm-4.5")` and `provider.kind = Some("zhipu")`

#### Scenario: Missing config file returns defaults
- **WHEN** no `.agent/config.toml` exists
- **THEN** `Config::load()` returns `Config::default()` without error

### Requirement: System prompts for Orchestrator and Task
The system SHALL provide identity system prompts: `orchestrator_prompt()` for foreground replies, `task_prompt()` for background Task agent loops. Task system prompt SHALL combine the identity prompt with the Focus Contract dimensions.

#### Scenario: Orchestrator foreground reply uses system prompt
- **WHEN** user sends a foreground message to the Orchestrator
- **THEN** `provider.stream()` is called with `system_prompt = Some(orchestrator_prompt())`

#### Scenario: Task system prompt includes focus dimensions
- **WHEN** a Task with focus `["security"]` builds its system prompt
- **THEN** the prompt contains both the Task identity text and the focus dimension list
