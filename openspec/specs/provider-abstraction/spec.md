## ADDED Requirements

### Requirement: Provider trait abstracts LLM access
The system SHALL define a Provider trait that abstracts LLM streaming. All agent logic SHALL depend on this trait, not on any concrete provider implementation. This is the linchpin of testability.

#### Scenario: Agent loop uses Provider trait
- **WHEN** Task A's agent loop needs an LLM response
- **THEN** it calls `provider.stream(messages, tools)` via the Provider trait
- **AND** no concrete provider name is hardcoded in the agent loop

### Requirement: ScriptedProvider for deterministic testing
The system SHALL provide a ScriptedProvider (test-only) that returns pre-programmed response sequences. It SHALL support chaining: `then_tool_call(name, args)`, `then_text(content)`, `then_tool_result(result)`, `then_done()`.

#### Scenario: Scripted tool call sequence
- **WHEN** a test uses ScriptedProvider with `.then_tool_call("read", "foo.rs").then_text("done")`
- **THEN** the agent loop receives the tool call first, then the text, then a done signal
- **AND** the test can assert on system state changes without real LLM calls

#### Scenario: ScriptedProvider exhausted
- **WHEN** the agent loop requests more responses than the ScriptedProvider was programmed with
- **THEN** an error is returned indicating the script was exhausted (test fails loudly)

### Requirement: Streaming via SSE
Provider implementations SHALL stream responses via Server-Sent Events (SSE), delivering incremental tokens, tool call deltas, and completion signals as a stream of events.

#### Scenario: Streaming text tokens
- **WHEN** Anthropic Provider streams a response
- **THEN** text tokens arrive incrementally as Stream events
- **AND** the agent loop processes them without waiting for the full response

### Requirement: Zero-copy context building with Cow
The system SHALL build LLM context (messages + tools) using `Cow<'_, [Message]>` and cached tool definitions to avoid unnecessary cloning. This is especially important for large sessions.

#### Scenario: Context built without cloning unchanged messages
- **WHEN** the agent loop builds context from 200 messages with 1 new message
- **THEN** the 199 unchanged messages are referenced via Cow (zero-copy)
- **AND** only the new message is newly allocated

### Requirement: Multiple provider implementations
The system SHALL provide provider implementations for at least: Anthropic (Claude), OpenAI (GPT), and Google (Gemini). Each SHALL support streaming, tool calling, and system prompt injection.

#### Scenario: Anthropic provider with tool calling
- **WHEN** AnthropicProvider.stream is called with messages and tool definitions
- **THEN** it returns a stream that may contain text deltas and tool_call events
- **AND** tool results can be fed back in subsequent calls
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
## ADDED Requirements

### Requirement: ProviderEvent::Usage for token tracking
The ProviderEvent enum SHALL include a `Usage { prompt_tokens: u64, completion_tokens: u64 }` variant. Provider implementations SHALL parse the `usage` field from the API response and emit this event before `Done`.

#### Scenario: OpenAI usage parsing
- **WHEN** OpenAIProvider receives a done event with `usage: { prompt_tokens: 120, completion_tokens: 340 }`
- **THEN** a `ProviderEvent::Usage { prompt_tokens: 120, completion_tokens: 340 }` is emitted before `Done`

#### Scenario: Provider without usage data
- **WHEN** a provider response has no usage field
- **THEN** no Usage event is emitted (the TUI simply doesn't update counts)

### Requirement: Message uses multi-part structure
The Message struct SHALL use a `parts: Vec<MessagePart>` field instead of flat `content`/`tool_calls` fields. Each part SHALL be one of: `Text(String)`, `Thinking(String)`, `ToolCall(ToolCall)`, or `ToolResult { tool_call_id, result }`. Message constructors (`user`, `assistant`, `system`, `tool_result`, `assistant_with_tools`) SHALL build the appropriate parts internally. Accessor methods (`text()`, `all_text()`, `thinking()`, `tool_calls()`, `tool_result_info()`) SHALL provide backward-compatible access patterns.

#### Scenario: Assistant message with thinking
- **WHEN** an LLM response contains reasoning followed by text
- **THEN** the Message has two parts: `Thinking("让我分析...")` and `Text("我来实现...")`
- **AND** `msg.thinking()` returns `Some("让我分析...")`
- **AND** `msg.text()` returns `"我来实现..."`

#### Scenario: Tool result message
- **WHEN** a tool result is constructed via `Message::tool_result("call-1", result)`
- **THEN** the Message has one part: `ToolResult { tool_call_id: "call-1", result }`
- **AND** `msg.tool_result_info()` returns `Some(("call-1", &result))`

### Requirement: ProviderEvent::ThinkingDelta for reasoning streams
The ProviderEvent enum SHALL include a `ThinkingDelta(String)` variant. Provider implementations SHALL parse reasoning/thinking content from the LLM stream and emit ThinkingDelta events.

#### Scenario: OpenAI-compatible reasoning_content
- **WHEN** the OpenAIProvider receives a delta with `reasoning_content: "让我分析..."`
- **THEN** a `ProviderEvent::ThinkingDelta("让我分析...")` is emitted

#### Scenario: Anthropic thinking_delta
- **WHEN** the AnthropicProvider receives a `thinking_delta` content block delta
- **THEN** a `ProviderEvent::ThinkingDelta(thinking_text)` is emitted

#### Scenario: Provider without reasoning support
- **WHEN** a provider response contains no reasoning/thinking fields
- **THEN** no ThinkingDelta events are emitted
