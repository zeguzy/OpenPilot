## ADDED Requirements

### Requirement: CLI input never blocks
The CLI frontend SHALL maintain a constantly-readable input loop. While background Tasks are running, the user SHALL be able to type and submit new messages immediately without waiting.

#### Scenario: Input while task runs
- **WHEN** Task A is running in the background
- **THEN** the user can type and submit a new message
- **AND** the new message is routed to Orchestrator without delay

### Requirement: Silent background mode by default
Background Tasks SHALL NOT produce output to the terminal by default. The user is informed of Task completion via notifications and can query progress at any time.

#### Scenario: Background task produces no output
- **WHEN** Task A is running and generating logs internally
- **THEN** no logs or intermediate output appear in the terminal
- **AND** the terminal remains available for user input

#### Scenario: Completion notification
- **WHEN** Task A completes
- **THEN** a notification line appears: `🔔 Ag: A done, modified 5 files`
- **AND** the notification does not interrupt user input

### Requirement: User can query task progress
The user SHALL be able to ask the Orchestrator about any Task's progress at any time. The Orchestrator responds using the latest heartbeat.

#### Scenario: Progress query
- **WHEN** user types "how is task A going?"
- **THEN** Orchestrator responds with Task A's latest heartbeat (state, progress, summary)

#### Scenario: List all active tasks
- **WHEN** user types "what's running?"
- **THEN** Orchestrator lists all active Tasks with their states and progress

### Requirement: Pending review indicator
When high-risk Tasks are pending review, the CLI SHALL show a subtle status indicator (e.g., "2 tasks pending review") without interrupting the user.

#### Scenario: Pending review shown
- **WHEN** 2 high-risk Tasks are completed but pending user review
- **THEN** a status line shows "2 tasks pending review" in the prompt area
- **AND** no popup or interruption occurs

### Requirement: User can accept or reject completed tasks
The user SHALL be able to accept (merge) or reject (discard/return) completed Tasks via simple commands.

#### Scenario: Accept task
- **WHEN** user types "accept task A"
- **THEN** Task A's changes are merged into the main project

#### Scenario: Reject task with feedback
- **WHEN** user types "reject task A, fix the token validation"
- **THEN** Task A is returned to OnIt with the feedback as a steering message
