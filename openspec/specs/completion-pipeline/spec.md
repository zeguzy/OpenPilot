## ADDED Requirements

### Requirement: Five-stage completion pipeline
When a Task reaches Delivered state, the system SHALL execute a five-stage pipeline: Freeze, Review, Merge, Memorialize, Cleanup. Each stage MUST complete (or fail gracefully) before the next begins.

#### Scenario: Normal completion flow
- **WHEN** Task A reaches Delivered state
- **THEN** the pipeline executes: Freeze → Review → Merge → Memorialize → Cleanup
- **AND** Task A ends in Archived state

### Requirement: Freeze stage locks workspace and generates summary
The Freeze stage SHALL make the workspace read-only, generate a final summary (either Agent self-summary or compressed by a small model), push a "Delivered" heartbeat, and notify Orchestrator and CLI.

#### Scenario: Freeze produces final summary
- **WHEN** Task A enters Freeze
- **THEN** workspace becomes read-only
- **AND** a final summary (~200 tokens) is generated
- **AND** Orchestrator and CLI receive a completion notification

### Requirement: Review stage uses risk-based grading (C+D hybrid)
The Review stage SHALL assess risk (based on diff size, file types, task type) and route accordingly: low-risk tasks get automated rule checks (compile/test); medium/high-risk tasks get an Audit Agent dispatched; the Orchestrator makes the final decision based on Audit report.

#### Scenario: Low-risk auto-accepted
- **WHEN** Task A's diff is 10 lines in a .md file
- **THEN** rule checks (compile/test) run automatically
- **AND** if checks pass, the Task is accepted without human review

#### Scenario: High-risk routed to Audit then human
- **WHEN** Task B's diff is 500 lines across 8 .rs files
- **THEN** an Audit Agent is dispatched with appropriate focus
- **AND** the Audit report is forwarded to the user with risk annotations for final approval

### Requirement: Merge stage detects and resolves conflicts
The Merge stage SHALL detect conflicts between the Task's changes and the current main project state. If conflicts exist, the Orchestrator SHALL attempt auto-resolve; if auto-resolve fails, the user SHALL be notified.

#### Scenario: Clean merge
- **WHEN** Task A merges and no conflicts exist
- **THEN** changes are applied to the main project and merge result is Clean

#### Scenario: Auto-resolvable conflict
- **WHEN** Task A and main project modified the same file but different sections
- **THEN** Orchestrator deep dives both contexts and attempts a merge resolution

#### Scenario: Unresolvable conflict escalated
- **WHEN** Orchestrator cannot auto-resolve a merge conflict
- **THEN** the user is notified with conflict details and asked to resolve manually

### Requirement: Memorialize stage archives to Cold Store
The Memorialize stage SHALL store the Task's final summary in Orchestrator's Active Memory, archive the full context and diff in Cold Store with indices, merge highlights into Orchestrator context, and trigger Orchestrator compaction if needed.

#### Scenario: Full context archived
- **WHEN** Task A enters Memorialize
- **THEN** Task A's full message history and diff are stored in Cold Store
- **AND** indices are built (keyword, time, task_id, tags, files modified)
- **AND** the final summary enters Orchestrator's Active Memory

### Requirement: Cleanup stage delays workspace removal
The Cleanup stage SHALL mark the workspace for delayed removal (configurable, default 3 days). Cold Store data SHALL NOT be affected by workspace cleanup.

#### Scenario: Workspace scheduled for cleanup
- **WHEN** Task A enters Cleanup
- **THEN** workspace is marked for removal after the delay period
- **AND** Cold Store data remains intact

### Requirement: Dependency chain auto-activates successor tasks
If a Task was dispatched with a dependency (e.g., "add 2FA after auth refactor"), when the predecessor completes and merges, the successor SHALL be automatically activated with a fresh workspace based on the updated main branch.

#### Scenario: Successor activated after merge
- **WHEN** Task A (refactor auth) completes and merges
- **THEN** Task B (add 2FA, which was waiting on A) is automatically activated
- **AND** Task B's workspace is created from the updated main branch

### Requirement: Completion notification respects user activity
When a Task completes while the user is occupied, low-risk completions SHALL be processed silently (auto-merge, result enters memory); high-risk completions SHALL be queued as "pending review" without interrupting the user.

#### Scenario: Low-risk completion while user busy
- **WHEN** Task A (low-risk) completes while user is typing a new message
- **THEN** Task A is auto-merged silently
- **AND** a subtle indicator shows "1 task completed" without interrupting

#### Scenario: High-risk completion queued
- **WHEN** Task B (high-risk) completes while user is busy
- **THEN** Task B is marked "pending review"
- **AND** a status indicator shows "1 task pending review" without a popup
