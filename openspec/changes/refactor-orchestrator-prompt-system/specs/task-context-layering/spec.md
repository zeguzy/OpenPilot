## MODIFIED Requirements

### Requirement: Layer 1 heartbeat is compact and frequent
Layer 1 heartbeats SHALL be emitted at most every 5 seconds and shall be under 50 tokens. Each Layer 1 heartbeat SHALL include: task ID, current TaskStatus, current phase (Phase 0/1/2/3), progress (0.0-1.0), short summary (max 30 tokens), current in-progress todo item (if any), and sub-task status summary (if any). When the task is `Waiting` for clarification, the heartbeat SHALL include the clarification question.

#### Scenario: Heartbeat includes in-progress todo
- **WHEN** Task A is executing Phase 2 with TodoList `[X, X, I, P, P]` (2 completed, 1 in-progress, 2 pending)
- **THEN** the Layer 1 heartbeat is `{task_id: A, status: OnIt, phase: 2, progress: 0.4, summary: "...", in_progress: "item 3 description"}`

#### Scenario: Heartbeat includes sub-task status
- **WHEN** Task A is `Waiting` with sub-tasks B and C running
- **THEN** the Layer 1 heartbeat includes `subtasks: [{id: B, status: OnIt, progress: 0.5}, {id: C, status: OnIt, progress: 0.2}]`

#### Scenario: Heartbeat includes clarification
- **WHEN** Task A is `Waiting` for user clarification
- **THEN** the Layer 1 heartbeat includes `clarification: "Should I migrate to JWT?"`

#### Scenario: Heartbeat respects frequency cap
- **WHEN** Task A produces 10 status updates in 1 second
- **THEN** at most 1 Layer 1 heartbeat is emitted in any 5-second window

## ADDED Requirements

### Requirement: Sub-task heartbeats aggregate to parent
When a parent Task has running sub-tasks, the parent's Layer 1 heartbeat SHALL include a compact summary of each sub-task's status. Sub-task Layer 2 highlights SHALL bubble up to the parent's Layer 2 only if they meet escalation criteria (severity `major` or `critical`); minor/info highlights stay local to the sub-task's history.

#### Scenario: Aggregated status in parent Layer 1
- **WHEN** Task A (parent) has sub-tasks B and C running
- **THEN** Task A's Layer 1 heartbeat shows their statuses inline

#### Scenario: Critical sub-task highlight escalates
- **WHEN** sub-task B emits a highlight with severity `critical`
- **THEN** the highlight is forwarded to Task A's Layer 2 stream with prefix `[subtask B]`

#### Scenario: Minor sub-task highlight does not escalate
- **WHEN** sub-task B emits a highlight with severity `info`
- **THEN** the highlight is recorded in B's history but does NOT appear in Task A's Layer 2 stream

### Requirement: Phase transitions emit Layer 2 highlights
When a Task transitions between phases (Phase 0 → Phase 1, Phase 1 → Phase 2, etc.), the Task SHALL emit a Layer 2 highlight noting the transition and the phase's outcome. This gives the Orchestrator visibility into the Task's reasoning cadence, not just tool-use activity.

#### Scenario: Phase 0 to Phase 1 transition
- **WHEN** Task A classifies the request as "explicit" in Phase 0 and moves to Phase 1
- **THEN** a Layer 2 highlight is emitted: `{tag: phase-transition, severity: info, summary: "Phase 0 → Phase 1: classified as explicit"}`

#### Scenario: Phase 1 assessment result
- **WHEN** Task A completes Phase 1 codebase assessment and decides the codebase is "disciplined"
- **THEN** a Layer 2 highlight is emitted with the assessment result and the patterns Task A will follow
