## MODIFIED Requirements

### Requirement: Orchestrator routes user messages
The Orchestrator SHALL NOT use keyword-based routing. All messages are sent to the LLM via stream_foreground. The LLM determines whether to dispatch a background task by including the `OPCA_DISPATCH:` prefix in its response.

#### Scenario: LLM decides to dispatch
- **WHEN** user says "refactor the auth module"
- **THEN** the LLM (not keyword matching) decides to include OPCA_DISPATCH prefix
- **AND** the Orchestrator dispatches a background Task based on that signal

#### Scenario: LLM decides to answer directly
- **WHEN** user says "what does this function do?"
- **THEN** the LLM responds without OPCA_DISPATCH prefix
- **AND** no background Task is created
