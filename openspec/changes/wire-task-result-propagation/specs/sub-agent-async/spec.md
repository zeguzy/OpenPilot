## MODIFIED Requirements

### Requirement: Subtask notification queue bridges child completion to parent
The Orchestrator SHALL maintain a `SubTaskNotificationQueue` per parent Task. When a child Task reaches a terminal state (Delivered/Stuck/Error), the Orchestrator SHALL construct a `SubTaskNotification` containing an enriched `SubTaskResult` and push it to the parent's queue. The `SubTaskResult` SHALL include:

- `task_id`: the child Task's identifier
- `summary`: the child's latest heartbeat summary (unchanged from prior behavior)
- `final_message`: `Option<String>` — the child's final assistant Message from `TaskOutcome::Completed`, or `None` if the child did not complete successfully
- `highlights`: `Vec<Highlight>` — the highlights the child recorded in the registry during its lifetime (not just the latest)
- `artifacts`: `Vec<PathBuf>` — paths the child modified, extracted from `Workspace::ChangeSet`; empty Vec if extraction fails or the child touched no files

The parent's run loop SHALL drain the queue at the start of each turn and inject each notification's result as a structured multi-line `Message::user(...)` into active messages. The injection format SHALL surface all non-empty fields so the parent LLM can reason about the child's deliverable, findings, and artifact footprint.

#### Scenario: Enriched notification pushed on child completion
- **WHEN** child Task "subtask-0" reaches Delivered with `TaskOutcome::Completed("auth module analyzed: 3 files...")`
- **AND** the child recorded 2 highlights during its run
- **AND** the child modified 3 files (reflected in its `Workspace::ChangeSet`)
- **THEN** a `SubTaskNotification::Completed` is pushed to the parent's queue
- **AND** `SubTaskResult.final_message == Some("auth module analyzed: 3 files...")`
- **AND** `SubTaskResult.highlights.len() == 2`
- **AND** `SubTaskResult.artifacts.len() == 3`

#### Scenario: Child stuck produces minimal enrichment
- **WHEN** child Task "subtask-1" reaches Stuck with reason "compile error"
- **THEN** a `SubTaskNotification::Failed` is pushed to the parent's queue
- **AND** no `SubTaskResult` is constructed (failure path uses reason string)
- **AND** the parent receives `[Sub-task error] subtask-1: compile error`

#### Scenario: Parent drains notifications on wake
- **WHEN** parent is woken from Waiting and enters a new turn
- **THEN** all pending notifications in the queue are drained
- **AND** each notification's enriched result is injected as a multi-line `Message::user(...)` block
- **AND** the block includes `final_message`, `highlights`, and `artifacts` sections when non-empty

#### Scenario: Old-format SubTaskResult still deserialises
- **WHEN** a `SubTaskResult` serialised before this change (only `task_id`, `summary`, `artifacts: []`) is loaded
- **THEN** deserialisation succeeds
- **AND** `final_message` defaults to `None`
- **AND** `highlights` defaults to an empty Vec
