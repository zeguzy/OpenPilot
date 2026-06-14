## ADDED Requirements

### Requirement: Task follows personified lifecycle state machine
Each Task SHALL progress through a state machine with the following states: Sleeping, Waking, Pondering, OnIt, Waiting, Reviewing, Delivered, Stuck, Axed, Archived. Only valid transitions are allowed; invalid transitions MUST be rejected.

#### Scenario: Normal lifecycle progression
- **WHEN** a Task is created and initialized
- **THEN** it transitions Sleeping → Waking → Pondering → OnIt → Delivered → Reviewing → Archived

#### Scenario: Invalid transition rejected
- **WHEN** a Task in state Sleeping receives a transition to Delivered
- **THEN** the transition is rejected and an error is returned

#### Scenario: Task gets stuck
- **WHEN** a Task in OnIt encounters an unrecoverable error
- **THEN** it transitions to Stuck and a heartbeat is pushed to Orchestrator

#### Scenario: Task cancelled from any state
- **WHEN** Orchestrator or user cancels a Task in OnIt
- **THEN** the Task transitions to Axed, then to Archived after cleanup

### Requirement: State transitions push heartbeat automatically
Whenever a Task's state changes, a heartbeat (Layer 1 context) SHALL be automatically pushed to the Orchestrator. The heartbeat contains: state, progress percentage, and a one-line summary of current activity.

#### Scenario: Heartbeat on state change
- **WHEN** Task A transitions from Pondering to OnIt with 0% progress
- **THEN** a heartbeat `{"state": "on-it", "progress": 0, "summary": "starting execution"}` is pushed to Orchestrator

### Requirement: Task panic converts to Crashed state
If a Task's tokio task panics, the Task state SHALL be set to a terminal error state and the Orchestrator and user SHALL be notified. The Task's Memory and workspace state MUST remain recoverable.

#### Scenario: Task panic during execution
- **WHEN** Task B's tokio task panics during OnIt
- **THEN** Task B's state becomes a terminal error state
- **AND** Orchestrator receives a notification with the panic message
- **AND** Task B's workspace and Memory remain intact for inspection

### Requirement: Task Waiting state pauses execution
When a Task needs input from Orchestrator or user, it SHALL transition to Waiting. A steering reply transitions it back to OnIt.

#### Scenario: Task waits for clarification
- **WHEN** Task C encounters an ambiguity it cannot resolve
- **THEN** it transitions to Waiting and pushes a heartbeat with the question
- **AND** when Orchestrator sends a steering reply, Task C transitions back to OnIt
