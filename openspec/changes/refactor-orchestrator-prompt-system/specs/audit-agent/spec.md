## ADDED Requirements

### Requirement: Audit Agent executes DoneClaim verification commands
When auditing a Task that is part of a continuation chain, the Audit Agent SHALL actively verify the Task's DoneClaim by executing claimed commands (e.g., if the Task claims "ran `cargo test` and all passed", Audit runs `cargo test` in the workspace) and checking that claimed artifacts exist. Discrepancies between claim and reality SHALL influence the verdict per the decision tree.

#### Scenario: Claim matches reality
- **WHEN** Task A claims "modified `src/auth.rs`, ran `cargo test auth` (all passed)" and Audit confirms both
- **THEN** Audit's verdict leans toward `Confirmed`

#### Scenario: Claim contradicted by reality
- **WHEN** Task A claims "all tests pass" but Audit's `cargo test` run shows 2 failing tests
- **THEN** Audit's verdict is `FalsePositive` or `NeedsFix` (per decision tree), with findings citing the failing test names

#### Scenario: Claim references missing artifact
- **WHEN** Task A's DoneClaim references a manual QA artifact at `qa/auth-checklist.md` that does not exist in the workspace
- **THEN** Audit notes the discrepancy in findings and the verdict reflects the unverified claim

### Requirement: Audit Agent prompt enforces verdict self-justification
Before emitting a verdict, the Audit Agent SHALL cite which finding(s) drove the verdict and why. This is enforced structurally by requiring a `justification` field in the Audit report (separate from `summary`). The Orchestrator SHALL surface the justification when overriding an Audit verdict, so the user can see why Audit made its initial call.

#### Scenario: Justification present in report
- **WHEN** Audit returns `NeedsFix` for Task A
- **THEN** the report includes `justification: "Two major findings in src/auth.rs:42 and tests/auth_test.rs:18; per decision tree, any major finding triggers NeedsFix"`

#### Scenario: Override shows original justification
- **WHEN** the Orchestrator overrides Audit's `NeedsFix` to `Confirmed`
- **THEN** the user-visible notification includes Audit's original justification so the user understands the override rationale
