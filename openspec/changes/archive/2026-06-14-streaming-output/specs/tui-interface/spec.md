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
