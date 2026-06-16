## MODIFIED Requirements

### Requirement: Orchestrator routes user messages
The Orchestrator SHALL analyze each user message and decide whether to handle it in the foreground (quick reply) or dispatch it as a background Task. The routing decision MUST consider the Orchestrator's active memory, including recent Task outcomes (`Delivered`, `Stuck`, `Axed`) and the count of pending Tasks. The routing decision MUST NOT block the user from sending subsequent messages.

When keyword-based routing is used as a fallback or tie-breaker, the keyword match result SHALL be refined by a context-aware tie-breaker that considers: (a) recency of the last dispatch, (b) follow-up markers in the message (`也`, `另外`, `顺便`, `and`, `also`, `then`, `接着`), and (c) overlap between the message and the summaries of recently failed Tasks. When LLM-based routing is used, the LLM SHALL receive the active-memory digest (recent outcomes, pending count) as part of its context, making the routing decision context-aware by construction.

#### Scenario: Quick question routed to foreground
- **WHEN** user asks "what does function X do?"
- **THEN** Orchestrator responds directly without creating a Task, response arrives within seconds

#### Scenario: Long task routed to background
- **WHEN** user says "refactor the auth module to OAuth2"
- **THEN** Orchestrator dispatches a background Task and immediately returns a task acknowledgment to the user

#### Scenario: User continues chatting while task runs
- **WHEN** a background Task is running AND user sends a new message
- **THEN** the new message is processed by Orchestrator without waiting for the Task to complete

#### Scenario: Follow-up to recent dispatch routed to foreground
- **WHEN** Orchestrator dispatched Task A within the last 60 seconds
- **AND** user sends a short message (< 100 chars) starting with a follow-up marker (e.g. "那顺便也把测试加上")
- **THEN** the message is routed to Foreground (treated as steering for Task A or an inline reply)
- **AND** no new Task is dispatched

#### Scenario: Re-ask about stuck task routed to background
- **WHEN** a recent Task outcome has `terminal_status = Stuck`
- **AND** its summary has bigram overlap > 0.3 with the user's new message
- **THEN** the message is routed to Background
- **AND** the new Task inherits focus dimensions from the failed Task

### Requirement: Orchestrator aggregates Task state via heartbeats
The Orchestrator SHALL maintain a registry of all active Tasks, updated by heartbeat pushes. The Orchestrator's Active Memory SHALL include, for each Task: the latest heartbeat (status + progress + summary), and upon terminal transition, the Task's final outcome (from `TaskOutcome::Completed`) and a bounded sample of promoted Task outputs (failed tool results and first-occurrence tool calls per tool name). Highlight `detail` payloads SHALL be preserved alongside their `summary` in Active Memory.

#### Scenario: Heartbeat received
- **WHEN** Task A transitions from Pondering to OnIt
- **THEN** Orchestrator's Task registry updates Task A's status to OnIt
- **AND** the heartbeat content is reflected in Orchestrator's Active Memory

#### Scenario: User queries task progress
- **WHEN** user asks "how is task A going?"
- **THEN** Orchestrator responds using the latest heartbeat from Task A

#### Scenario: Task completion outcome enters memory
- **WHEN** Task A's join handle resolves to `TaskOutcome::Completed(msg)`
- **AND** the Orchestrator's poll loop calls `poll_task_outcome("task-A")`
- **THEN** an `OrchestratorEvent` with `EventKind::Delivered` is pushed to Active Memory
- **AND** the event's text contains the completed Message content
- **AND** the event is exempt from archival compaction

#### Scenario: Tool failure promoted to memory
- **WHEN** Task A emits `TaskOutput::ToolResult { success: false, .. }`
- **THEN** an `OrchestratorEvent` with `EventKind::ToolActivity { success: false }` is pushed to Active Memory
- **AND** compaction retains the last 3 such events per Task

#### Scenario: First tool call promoted, subsequent de-duplicated
- **WHEN** Task A emits `TaskOutput::ToolCall { name: "ReadTool", .. }` for the first time
- **THEN** an `OrchestratorEvent` with `EventKind::ToolActivity { success: true }` is pushed to Active Memory
- **WHEN** Task A emits `TaskOutput::ToolCall { name: "ReadTool", .. }` again
- **THEN** no new event is pushed (de-duplicated by tool name within the Task)

#### Scenario: Promoted output cap enforced
- **WHEN** Task A already has `max_promoted_outputs_per_task` (default 8) `ToolActivity` events in Active Memory
- **AND** a new promotable output arrives
- **THEN** the oldest `ToolActivity` event for Task A is archived to the SQLite store
- **AND** the new event takes its place in Active Memory

### Requirement: Orchestrator detects child completion and notifies parent
The Orchestrator SHALL check for child Tasks that have reached a terminal state (Delivered/Stuck/Error) whose parent is in Waiting. For each such child, the Orchestrator SHALL construct an enriched `SubTaskResult` containing: (a) the child's latest heartbeat summary, (b) the child's final message from `TaskOutcome::Completed` (if available), (c) the child's recorded highlights (copied from the registry), and (d) the child's artifact paths (extracted from `Workspace::ChangeSet`). The Orchestrator SHALL inject a structured multi-line message into the parent's steering channel.

#### Scenario: Child delivered, parent woken with enriched result
- **WHEN** child Task "task-5" (parent="task-3") reaches Delivered with `TaskOutcome::Completed("重构了 auth 模块...")`
- **AND** parent "task-3" is in Waiting
- **THEN** Orchestrator constructs `SubTaskResult { summary, final_message: Some("重构了 auth 模块..."), highlights: [...], artifacts: [...] }`
- **AND** injects a multi-line `[Sub-task result]` block into task-3's steering channel
- **AND** task-3 transitions back to OnIt on its next steering poll

#### Scenario: Child stuck, parent notified with reason
- **WHEN** child Task "task-5" (parent="task-3") reaches Stuck with reason "compile error"
- **AND** parent "task-3" is in Waiting
- **THEN** Orchestrator injects `Message::user("[Sub-task error] compile error")` into task-3's steering channel
- **AND** `SubTaskResult.final_message` is `None` (no Completed outcome)

#### Scenario: Child delivered without highlights or artifacts
- **WHEN** child Task "task-5" reaches Delivered but recorded no highlights and touched no files
- **THEN** `SubTaskResult.highlights` is an empty Vec
- **AND** `SubTaskResult.artifacts` is an empty Vec
- **AND** `SubTaskResult.final_message` still contains the completed Message

## ADDED Requirements

### Requirement: Orchestrator preserves Highlight detail in memory
When the Orchestrator drains highlights from a Task's highlight channel, it SHALL include both the `summary` and the optional `detail` field in the resulting `OrchestratorEvent`. The `detail` payload SHALL NOT be silently dropped. When `detail` is present, the event text SHALL use a delimiter (e.g. `\n---\n`) between summary and detail to keep them greppable.

#### Scenario: Highlight with detail fully preserved
- **WHEN** Task A emits `Highlight { tag: "security", severity: Warning, summary: "hardcoded secret", detail: Some("found in src/auth.rs:42, leaked via...") }`
- **THEN** the resulting `OrchestratorEvent` text contains both the summary and the detail
- **AND** the detail is searchable via the standard `recall` keyword query

#### Scenario: Highlight without detail unchanged
- **WHEN** Task A emits `Highlight { tag: "security", summary: "minor issue", detail: None }`
- **THEN** the resulting `OrchestratorEvent` text matches the pre-change format `"[security] minor issue"`

### Requirement: Orchestrator polls TaskOutcome non-blockingly on terminal transitions
The Orchestrator SHALL expose a `poll_task_outcome(task_id)` method that attempts to claim the Task's `JoinHandle` without blocking. On terminal status transitions (`Delivered | Archived | Stuck | Axed`), the Orchestrator's turn loop SHALL call this method for every Task whose outcome has not yet been recorded. If the Task has not yet finished, the method SHALL return `None` and leave the join handle in place for a later poll.

#### Scenario: Outcome claimed on first poll
- **WHEN** Task A's status transitions to Delivered
- **AND** the join handle has already resolved
- **THEN** `poll_task_outcome("task-A")` returns `Some(Completed(msg))`
- **AND** a `Delivered` event is pushed to memory
- **AND** the join handle is consumed (subsequent polls return `None`)

#### Scenario: Outcome not yet ready
- **WHEN** Task A's status is Delivered in the registry (heartbeat-driven) but the join handle has not yet resolved
- **THEN** `poll_task_outcome("task-A")` returns `None`
- **AND** the join handle remains in the registry for a later poll
- **AND** no `Delivered` event is pushed yet

#### Scenario: Outcome for already-recorded task
- **WHEN** `poll_task_outcome("task-A")` has previously returned `Some(...)` for Task A
- **THEN** subsequent calls return `None` without re-pushing the event
