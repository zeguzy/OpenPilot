## ADDED Requirements

### Requirement: stream_foreground method on OrchestratorApi
The OrchestratorApi trait SHALL include `stream_foreground(message, sender)` which spawns an async task that calls the Provider and pushes StreamEvent::Delta for each TextDelta, StreamEvent::Done on completion, or StreamEvent::Error on failure.

#### Scenario: Real provider streams
- **WHEN** RealOrchestrator.stream_foreground is called
- **THEN** a tokio task is spawned that calls provider.stream() and pushes each TextDelta to the channel

#### Scenario: Mock provider streams word by word
- **WHEN** MockOrchestrator.stream_foreground is called
- **THEN** the echo response is pushed word by word with 30ms delay
