## ADDED Requirements

### Requirement: Orchestrator routes user messages
The Orchestrator SHALL analyze each user message and decide whether to handle it in the foreground (quick reply) or dispatch it as a background Task. The routing decision MUST NOT block the user from sending subsequent messages.

#### Scenario: Quick question routed to foreground
- **WHEN** user asks "what does function X do?"
- **THEN** Orchestrator responds directly without creating a Task, response arrives within seconds

#### Scenario: Long task routed to background
- **WHEN** user says "refactor the auth module to OAuth2"
- **THEN** Orchestrator dispatches a background Task and immediately returns a task acknowledgment to the user

#### Scenario: User continues chatting while task runs
- **WHEN** a background Task is running AND user sends a new message
- **THEN** the new message is processed by Orchestrator without waiting for the Task to complete

### Requirement: Orchestrator dispatches Tasks with Focus Contract
When dispatching a background Task, the Orchestrator SHALL attach a Focus Contract specifying which dimensions the Task must report on (e.g., "security risks", "breaking API changes").

#### Scenario: Dispatch with focus
- **WHEN** Orchestrator dispatches Task "refactor auth"
- **THEN** the Task receives a focus list like ["security risks", "breaking changes", "tradeoff decisions"]
- **AND** the focus list is injected into the Task's system prompt

### Requirement: Orchestrator aggregates Task state via heartbeats
The Orchestrator SHALL maintain a registry of all active Tasks, updated by heartbeat pushes. The Orchestrator's Active Memory includes each Task's latest heartbeat (status + progress + one-line summary).

#### Scenario: Heartbeat received
- **WHEN** Task A transitions from Pondering to OnIt
- **THEN** Orchestrator's Task registry updates Task A's status to OnIt
- **AND** the heartbeat content is reflected in Orchestrator's Active Memory

#### Scenario: User queries task progress
- **WHEN** user asks "how is task A going?"
- **THEN** Orchestrator responds using the latest heartbeat from Task A

### Requirement: Orchestrator can deep dive into Task context
The Orchestrator SHALL have a `deep_dive(task_id, query)` tool to retrieve relevant fragments from a Task's Layer 3 full context. This access is read-only and returns a snapshot.

#### Scenario: Deep dive for stuck task
- **WHEN** Task B is Stuck and Orchestrator calls deep_dive("task-B", "last 10 messages")
- **THEN** the last 10 messages from Task B's full context are returned as a read-only snapshot
- **AND** the snapshot is temporarily added to Orchestrator's Active Memory

### Requirement: Orchestrator predicts dispatch conflicts
Before dispatching Tasks, the Orchestrator SHALL perform a lightweight conflict prediction by estimating which files each Task will touch. Tasks with overlapping file sets MUST be serialized.

#### Scenario: Non-overlapping tasks dispatched in parallel
- **WHEN** Task A targets src/auth/* and Task B targets src/utils/*
- **THEN** both Tasks are dispatched simultaneously with separate workspaces

#### Scenario: Overlapping tasks serialized
- **WHEN** Task A targets src/auth/* and Task C targets src/auth/*
- **THEN** Task C is queued and only dispatched after Task A completes and merges

### Requirement: Orchestrator dynamically adjusts Focus Contract
The Orchestrator SHALL be able to update a running Task's Focus Contract via steering, adding or removing focus dimensions. The hard cap is 8 dimensions; removing is required before adding beyond the cap.

#### Scenario: Add focus dimension
- **WHEN** Orchestrator sends update_focus to Task A with add=["performance impact"]
- **THEN** Task A's next turn sees the updated focus list including "performance impact"

#### Scenario: Focus cap enforced
- **WHEN** Task A already has 8 focus dimensions AND Orchestrator tries to add a 9th
- **THEN** the addition is rejected with an error indicating the cap is reached
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
