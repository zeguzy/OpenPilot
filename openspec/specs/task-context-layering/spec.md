## ADDED Requirements

### Requirement: Task maintains three-layer context
Each Task SHALL maintain three layers of context: Layer 1 Heartbeat (auto, ~50 tokens), Layer 2 Highlights (agent-reported, ~100-300 tokens each), Layer 3 Full (complete message history). The Orchestrator SHALL only see Layer 1 and 2 by default.

#### Scenario: Layer 1 heartbeat auto-generated
- **WHEN** Task A completes a turn
- **THEN** a Layer 1 heartbeat is generated and pushed to Orchestrator automatically

#### Scenario: Layer 3 not auto-pushed
- **WHEN** Task A accumulates 50 messages in its full context
- **THEN** none of the Layer 3 content is pushed to Orchestrator automatically
- **AND** Layer 3 is only accessible via Orchestrator's deep_dive tool

### Requirement: report_highlight tool for Layer 2
The Task SHALL have a `report_highlight` tool that the Agent can call to report important findings. Each highlight MUST include a tag matching a current Focus Contract dimension, a severity (info/warning/blocking), and a summary.

#### Scenario: Agent reports security finding
- **WHEN** Agent calls report_highlight(tag="security risks", severity="warning", summary="hardcoded client_secret found")
- **THEN** the highlight is added to Task's Layer 2 context
- **AND** the highlight is pushed to Orchestrator immediately

#### Scenario: Highlight tag not in focus contract
- **WHEN** Agent calls report_highlight(tag="documentation", ...) but "documentation" is not in the focus list
- **THEN** the call returns an error indicating the tag is not in the current focus contract

### Requirement: Focus Contract bound at dispatch
When a Task is dispatched, the Orchestrator SHALL attach a Focus Contract (list of dimension strings). The focus dimensions are injected into the Task's system prompt.

#### Scenario: Focus contract included in system prompt
- **WHEN** Task A is dispatched with focus ["security risks", "breaking changes"]
- **THEN** Task A's system prompt contains instructions to report on those dimensions

### Requirement: Focus Contract dynamically adjustable
The Orchestrator SHALL update a running Task's Focus Contract via steering (add/remove dimensions). The Task's next turn SHALL see the updated focus list.

#### Scenario: Remove focus dimension
- **WHEN** Orchestrator sends update_focus(remove=["breaking changes"]) to Task A
- **THEN** Task A's focus list no longer contains "breaking changes"
- **AND** subsequent report_highlight calls with tag="breaking changes" are rejected

### Requirement: Focus Contract hard cap at 8 dimensions
The Focus Contract SHALL enforce a maximum of 8 dimensions. Adding beyond 8 requires removing first.

#### Scenario: Cap enforced on add
- **WHEN** Task has 8 focus dimensions and update_focus(add=["new dim"]) is called
- **THEN** the add is rejected with a cap-exceeded error

#### Scenario: Remove then add within cap
- **WHEN** Task has 8 dimensions and update_focus(remove=["old dim"], add=["new dim"]) is called
- **THEN** the update succeeds, resulting in 8 dimensions with "new dim" replacing "old dim"
