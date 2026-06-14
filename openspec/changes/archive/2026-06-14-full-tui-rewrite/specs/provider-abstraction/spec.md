## ADDED Requirements

### Requirement: ProviderEvent::Usage for token tracking
The ProviderEvent enum SHALL include a `Usage { prompt_tokens: u64, completion_tokens: u64 }` variant. Provider implementations SHALL parse the `usage` field from the API response and emit this event before `Done`.

#### Scenario: OpenAI usage parsing
- **WHEN** OpenAIProvider receives a done event with `usage: { prompt_tokens: 120, completion_tokens: 340 }`
- **THEN** a `ProviderEvent::Usage { prompt_tokens: 120, completion_tokens: 340 }` is emitted before `Done`

#### Scenario: Provider without usage data
- **WHEN** a provider response has no usage field
- **THEN** no Usage event is emitted (the TUI simply doesn't update counts)
