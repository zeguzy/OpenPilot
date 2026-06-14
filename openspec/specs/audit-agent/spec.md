## ADDED Requirements

### Requirement: Audit Agent is a read-only Task specialization
The Audit Agent SHALL be a Task variant with role=Auditor, readonly workspace access, simplified spawn-die lifecycle, and no worktree of its own. It reads the audited Task's diff and workspace.

#### Scenario: Audit spawned for completed task
- **WHEN** Task A (medium-risk) reaches Delivered
- **THEN** an Audit Agent is spawned with read-only access to Task A's workspace and diff
- **AND** the Audit Agent has no worktree of its own

### Requirement: Audit lifecycle is minimal (Spawn → Inspect → Judge → Report → Die)
The Audit Agent SHALL follow a minimal lifecycle without the full personified state machine. It spawns, inspects the Task's output, renders a judgment, reports, and terminates.

#### Scenario: Audit completes and dies
- **WHEN** Audit Agent finishes inspecting Task A's diff
- **THEN** it produces a report and terminates—no Archived state, no worktree cleanup needed

### Requirement: Audit focus inherits from Task focus plus standards
The Audit Agent's focus SHALL include: dimensions inherited from the audited Task's Focus Contract, standard checks (compilation, tests, diff sanity), and any additional dimensions specified by the Orchestrator.

#### Scenario: Focus inheritance
- **WHEN** Task A had focus ["security risks", "breaking changes"]
- **THEN** Audit Agent's focus includes ["security risks", "breaking changes", "compilation", "tests", "diff sanity"]

### Requirement: Audit default mode is black-box
The Audit Agent SHALL default to black-box mode: only examining the diff, final summary, and running tests/compilation. The Audit Agent SHALL have a `deep_dive_task_context` tool to optionally inspect the Task's full reasoning process when the diff raises suspicion.

#### Scenario: Black-box audit passes
- **WHEN** Audit Agent reviews diff and all tests pass
- **THEN** verdict is "pass" with high confidence

#### Scenario: Deep dive triggered by suspicious diff
- **WHEN** Audit Agent finds a function deletion suspicious in the diff
- **THEN** it calls deep_dive_task_context to check the Task's reasoning for the deletion
- **AND** if the reasoning is flawed, the verdict reflects "fail"

### Requirement: Audit report is structured
The Audit Agent SHALL produce a structured report with: verdict (pass/warn/fail), confidence (0.0-1.0), findings list (severity + location + issue), and a summary string.

#### Scenario: Warn verdict with findings
- **WHEN** Audit Agent finds a minor issue but overall the change is acceptable
- **THEN** report is `{verdict: "warn", confidence: 0.7, findings: [{severity: "minor", ...}], summary: "..."}`

### Requirement: Orchestrator can override Audit verdict
The Orchestrator is the decision-maker and SHALL be able to override the Audit Agent's verdict. For example, if Audit reports "fail" due to test failures, but the Orchestrator knows those tests were pre-existing failures, it can override to "accept".

#### Scenario: Override fail to accept
- **WHEN** Audit reports "fail" (tests failed) but Orchestrator recalls those tests were already broken
- **THEN** Orchestrator overrides to "accept" and proceeds with merge

### Requirement: Audit cost control via model and turn limits
The Audit Agent SHALL use a cheaper model (haiku/flash tier) by default and SHALL be limited to 1-3 turns. High-risk tasks MAY use a stronger model via configuration.

#### Scenario: Low-risk uses cheap model
- **WHEN** a medium-risk task triggers Audit
- **THEN** the Audit Agent uses the configured cheap model (e.g., haiku)

#### Scenario: Audit turn limit enforced
- **WHEN** Audit Agent reaches its 3rd turn without a verdict
- **THEN** it is forced to produce a report with reduced confidence
