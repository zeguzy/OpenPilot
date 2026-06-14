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
