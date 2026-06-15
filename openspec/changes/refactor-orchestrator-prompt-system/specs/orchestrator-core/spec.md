## ADDED Requirements

### Requirement: Orchestrator dispatches Tasks via tool call
The Orchestrator SHALL dispatch background Tasks by emitting an explicit `dispatch_task` tool call (with prompt, focus dimensions, and optional predecessor dependency). The Orchestrator SHALL NOT use string-prefix-based dispatch triggers (e.g., the legacy `OPCA_DISPATCH:` prefix). Routing is deterministic on tool calls, not best-effort string matching against model output.

#### Scenario: Orchestrator dispatches via tool call
- **WHEN** the user asks "refactor the auth module" and the Orchestrator decides this is long-running work
- **THEN** the Orchestrator emits a `dispatch_task` tool call with `prompt="refactor the auth module"` and `focus=["compilation", "tests"]`
- **AND** the resulting Task is dispatched via the standard `dispatch_task` API

#### Scenario: Quick reply in foreground
- **WHEN** the user asks "what does the auth module do?"
- **THEN** the Orchestrator responds directly in the foreground without emitting `dispatch_task`
- **AND** no Task is spawned

#### Scenario: Legacy prefix ignored
- **WHEN** the Orchestrator's output contains the literal string `OPCA_DISPATCH: refactor auth`
- **THEN** the string is treated as ordinary text and shown to the user
- **AND** no Task is dispatched from this string

### Requirement: Orchestrator surfaces Task clarification requests to user
When a Task transitions to `Waiting` state with a clarification question, the Orchestrator SHALL surface the question to the user via a structured notification (not buried in the output stream). The notification SHALL include: Task ID, Task summary, the clarification question, and a hint that the user can answer via `/answer <task-id> <response>`.

#### Scenario: Clarification notification
- **WHEN** Task A enters `Waiting` with question "Should I migrate to JWT or keep sessions?"
- **THEN** the Orchestrator emits a structured notification: "[Task A waiting] Question: Should I migrate to JWT or keep sessions? Reply with: /answer task-a <your choice>"

#### Scenario: User answers via slash command
- **WHEN** the user types `/answer task-a JWT please`
- **THEN** the Orchestrator forwards the response to Task A as a `SteeringMessage::Inject`
- **AND** Task A transitions out of `Waiting`

#### Scenario: Clarification timeout auto-proceeds
- **WHEN** Task A has been waiting 5 minutes without a user response
- **THEN** the Orchestrator notifies the user "Task A clarification timed out, proceeding with best-guess interpretation"
- **AND** Task A resumes execution with its recorded best-guess

### Requirement: Orchestrator enforces Tone & Communication policy
The Orchestrator's user-facing responses SHALL follow the Tone policy specified in its prompt: no flattery ("Great question!"), no status acknowledgments ("Let me start..."), concise direct answers, raising concerns about suboptimal approaches before implementing. The Orchestrator is the ONLY role that produces user-visible text; this policy is enforced structurally.

#### Scenario: No flattery
- **WHEN** the user asks "how do I configure auth?"
- **THEN** the Orchestrator responds directly with the configuration steps
- **AND** the response does NOT start with "Great question!" or similar praise

#### Scenario: Concern raised on flawed approach
- **WHEN** the user asks to implement something that contradicts codebase patterns
- **THEN** the Orchestrator responds with the concern-and-alternative format before implementing
- **AND** asks the user to confirm before proceeding

#### Scenario: Concise acknowledgment of multi-step work
- **WHEN** the user dispatches a long-running Task
- **THEN** the Orchestrator's reply is short ("Dispatched Task A: <summary>") and uses heartbeats for ongoing progress, not chatter

### Requirement: Orchestrator provides Context-Completion Gate
Before dispatching a Task, the Orchestrator SHALL verify that all of the following are true: (1) the request contains an explicit action verb (implement/add/fix/refactor/etc.), (2) the scope is concrete enough to execute without guessing, (3) no blocking specialist consultation is pending. If any condition fails, the Orchestrator enters a clarification round with the user instead of dispatching.

#### Scenario: Sufficient context dispatches
- **WHEN** the user says "implement JWT auth for the REST API in src/api/routes/"
- **THEN** the Orchestrator dispatches a Task (context is concrete: explicit verb, specific path)

#### Scenario: Insufficient context asks
- **WHEN** the user says "make it better"
- **THEN** the Orchestrator does NOT dispatch
- **AND** asks for clarification: "What specifically should be improved? Performance, code quality, API design, or something else?"

#### Scenario: Ambiguous scope asks
- **WHEN** the user says "add tests"
- **THEN** the Orchestrator asks "For which module or capability? Unit tests, integration tests, or both?"
