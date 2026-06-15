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
The Audit Agent SHALL produce a structured report with: verdict (`Confirmed` / `FalsePositive` / `NeedsFix` / `NeedsHumanReview`), confidence (0.0-1.0), findings list (severity + location + issue), and a summary string. The verdict drives continuation decisions: only `Confirmed` terminates a continuation chain; `FalsePositive` and `NeedsFix` trigger continuation with feedback; `NeedsHumanReview` halts the chain and escalates to the user.

#### Scenario: Confirmed verdict
- **WHEN** Audit Agent reviews the diff and all checks pass
- **THEN** report is `{verdict: "Confirmed", confidence: 0.95, findings: [], summary: "All tests pass, diff is clean"}`

#### Scenario: FalsePositive verdict
- **WHEN** Audit Agent reviews the diff and finds the Task claimed completion but a core function is still unimplemented
- **THEN** report is `{verdict: "FalsePositive", confidence: 0.8, findings: [{severity: "critical", location: "src/auth.rs:42", issue: "verify_token() body is empty"}], summary: "..."}`

#### Scenario: NeedsFix verdict with findings
- **WHEN** Audit Agent finds a failing test that the Task did not address
- **THEN** report is `{verdict: "NeedsFix", confidence: 0.9, findings: [{severity: "major", location: "tests/auth_test.rs", issue: "test_oauth_flow panics"}], summary: "..."}`

#### Scenario: NeedsHumanReview verdict
- **WHEN** Audit Agent confidence is below the configured threshold (e.g., 0.5) and the diff is ambiguous
- **THEN** report is `{verdict: "NeedsHumanReview", confidence: 0.4, findings: [...], summary: "Cannot determine correctness automatically"}`

### Requirement: Orchestrator can override Audit verdict
The Orchestrator is the decision-maker and SHALL be able to override the Audit Agent's verdict. For example, if Audit reports `NeedsFix` due to test failures, but the Orchestrator knows those tests were pre-existing failures, it can override to `Confirmed`. An override to `Confirmed` terminates the continuation chain; an override to any other verdict follows that verdict's normal continuation behavior.

#### Scenario: Override NeedsFix to Confirmed
- **WHEN** Audit reports `NeedsFix` (tests failed) but Orchestrator recalls those tests were already broken before this Task
- **THEN** Orchestrator overrides to `Confirmed` and the continuation chain terminates

#### Scenario: Override recorded with rationale
- **WHEN** the Orchestrator overrides an Audit verdict
- **THEN** the override is recorded in the chain's iteration history with the original verdict, the overridden verdict, and the Orchestrator's rationale

### Requirement: Audit cost control via model and turn limits
The Audit Agent SHALL use a cheaper model (haiku/flash tier) by default and SHALL be limited to 1-3 turns. High-risk tasks MAY use a stronger model via configuration.

#### Scenario: Low-risk uses cheap model
- **WHEN** a medium-risk task triggers Audit
- **THEN** the Audit Agent uses the configured cheap model (e.g., haiku)

#### Scenario: Audit turn limit enforced
- **WHEN** Audit Agent reaches its 3rd turn without a verdict
- **THEN** it is forced to produce a report with reduced confidence

### Requirement: Audit Agent spawned in production path
The Audit Agent SHALL be spawned via `AuditAgent::new` in the production CompletionPipeline Review stage for medium and high-risk Tasks. The Review stage SHALL NOT return hardcoded verdict stubs. The Audit Agent SHALL be spawned with read-only workspace access, the Task's diff, and the Task's focus dimensions.

#### Scenario: Medium-risk Task spawns Audit Agent
- **WHEN** Task A (medium-risk, 50-line diff in .rs files) reaches the Review stage
- **THEN** `AuditAgent::new` is called with Task A's workspace path, diff, and focus
- **AND** the Audit Agent runs and returns a structured report
- **AND** the Review stage does NOT return a hardcoded `Warn` stub

#### Scenario: Audit Agent uses configured cheap model
- **WHEN** Task A triggers Audit in production
- **THEN** the Audit Agent uses the configured cheap model tier (e.g., haiku/flash) by default
- **AND** high-risk Tasks MAY use a stronger model via configuration

### Requirement: Audit verifies DoneClaim against actual diff
When auditing a Task that is part of a continuation chain, the Audit Agent SHALL verify the Task's self-reported `DoneClaim` (claims about changed files, tests run, manual QA performed) against the actual diff and workspace state. Discrepancies between the claim and reality SHALL influence the verdict.

#### Scenario: DoneClaim matches reality
- **WHEN** Task A claims it modified `src/auth.rs` and ran `cargo test auth` successfully, and the diff confirms both
- **THEN** the Audit verdict leans toward `Confirmed`

#### Scenario: DoneClaim contradicts reality
- **WHEN** Task A claims "all tests pass" but the Audit Agent runs the tests and 2 fail
- **THEN** the Audit verdict is `FalsePositive` or `NeedsFix` with findings citing the failing tests

#### Scenario: DoneClaim references missing artifacts
- **WHEN** Task A's DoneClaim references a manual QA artifact that does not exist in the workspace
- **THEN** the Audit Agent notes the discrepancy in findings and the verdict reflects the unverified claim
