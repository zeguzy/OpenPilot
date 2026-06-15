## ADDED Requirements

### Requirement: Centralized prompt registry
The system SHALL maintain a single `prompt_system` module tree containing all prompt templates used by Orchestrator, Task, Audit Agent, Continuation Coordinator, and Focus Contract. No prompt template SHALL be inlined in business logic files (`task.rs`, `agent.rs`, `coordinator.rs`, etc.). All templates SHALL be exposed via `pub const` or `pub fn` accessors with doc comments explaining the template's purpose, intended role, and required variables.

#### Scenario: All prompts co-located
- **WHEN** a developer searches for any prompt template (orchestrator, task, audit, continuation, focus)
- **THEN** they find it under `crates/opca-core/src/prompt_system/` 
- **AND** no prompt string literals longer than 80 chars exist in `task/`, `audit/`, `continuation/`, `orchestrator/`, `focus/` business logic

#### Scenario: Prompt template is documented
- **WHEN** a developer opens a prompt template definition
- **THEN** they see a `///` doc comment explaining the prompt's purpose, the role it targets, and the variables it expects

### Requirement: Prompt template versioning
Each prompt template SHALL carry a version string. When a template's wording changes materially (not just whitespace), the version SHALL be incremented. The version is exposed alongside the template so consumers can log which prompt version produced a given model response. This enables A/B comparison and rollbacks.

#### Scenario: Template exposes version
- **WHEN** a Task is dispatched
- **THEN** the system records the prompt template version used (e.g., `"task_system": "v2"`) in the Task's initialization heartbeat

#### Scenario: Version bumps on material change
- **WHEN** a developer changes the Task prompt from "be thorough" to a 3-paragraph phase protocol
- **THEN** the prompt version constant is incremented (e.g., `TASK_SYSTEM_V1` → `TASK_SYSTEM_V2`)

### Requirement: Task prompt implements Phase Protocol
The Task system prompt SHALL structure the agent's behavior into named phases: Phase 0 (Intent Gate), Phase 1 (Codebase Assessment), Phase 2 (Execution), Phase 3 (Completion with Evidence Gate). Each phase SHALL have explicit entry conditions, exit conditions, and forbidden actions enumerated in the prompt text. The phase names match the phases enforced structurally by `Task::run_turn`.

#### Scenario: Phase 0 Intent Gate in prompt
- **WHEN** the Task system prompt is rendered
- **THEN** it contains a Phase 0 section instructing the model to classify the request as trivial/explicit/exploratory/open-ended/ambiguous and to ask one clarifying question if ambiguous

#### Scenario: Phase 1 Codebase Assessment in prompt
- **WHEN** the Task system prompt is rendered
- **THEN** it contains a Phase 1 section instructing the model to sample 2-3 similar files, classify codebase state (disciplined/transitional/legacy/greenfield), and record the classification in the first heartbeat

#### Scenario: Phase 3 Evidence Gate in prompt
- **WHEN** the Task system prompt is rendered
- **THEN** it contains a Phase 3 section enumerating the verification commands the model MUST run (build/test/clippy or project equivalent) before claiming completion

### Requirement: Task prompt enumerates Hard Blocks
The Task system prompt SHALL enumerate forbidden actions explicitly. For Rust-adjacent projects the list includes: `unsafe` code, `.unwrap()` in library code, `expect()` outside tests, `#[allow(clippy::...)]` without justification in commit message, `as Any`-style type erasure, empty `catch(e) {}`, leaving code in broken state after failures, deleting failing tests to "pass", shotgun debugging (random changes), `@ts-ignore` for TS-adjacent. The model is instructed to refuse these actions and explain why.

#### Scenario: Hard Blocks list present in Task prompt
- **WHEN** the Task system prompt is rendered for a Rust project
- **THEN** it contains a Hard Blocks section listing at minimum: `unsafe` code, `.unwrap()` in library code, `expect()` outside tests, unjustified clippy suppressions, broken-state-after-failures, test-deletion

#### Scenario: Model refuses Hard Block
- **WHEN** the model considers emitting `.unwrap()` in library code to bypass a type error
- **THEN** the Task prompt's Hard Blocks section causes the model to instead return a `Result` and propagate the error

### Requirement: Orchestrator prompt includes Tone & Communication policy
The Orchestrator system prompt SHALL include explicit guidance on user-facing communication: no flattery ("Great question!"), no status updates ("Let me start..."), concise direct answers, matching user's communication style, raising concerns about suboptimal user requests before implementing. This policy is enforced structurally because the Orchestrator is the only role that produces user-visible text.

#### Scenario: Tone policy in Orchestrator prompt
- **WHEN** the Orchestrator system prompt is rendered
- **THEN** it contains a Tone section with the specific forbidden phrasings listed above

#### Scenario: Orchestrator raises concern on flawed approach
- **WHEN** the user requests an implementation approach that contradicts established codebase patterns
- **THEN** the Orchestrator prompt's policy causes it to respond with "I notice [observation]. This might cause [problem] because [reason]. Alternative: [suggestion]. Should I proceed with your original request, or try the alternative?"

### Requirement: Audit prompt specifies judgment criteria
The Audit Agent system prompt SHALL specify, beyond the output format, the judgment criteria for each verdict: severity enum definitions (critical/major/minor/info), confidence bands (high ≥0.8, medium 0.5-0.8, low <0.5), and a decision tree mapping severity+confidence to verdict (Confirmed only when high+no critical; NeedsFix when any major/critical finding; NeedsHumanReview when low confidence; FalsePositive only when DoneClaim contradicts diff).

#### Scenario: Severity enum defined in prompt
- **WHEN** the Audit Agent system prompt is rendered
- **THEN** it contains a Severity section defining critical/major/minor/info with examples

#### Scenario: Verdict decision tree in prompt
- **WHEN** the Audit Agent system prompt is rendered
- **THEN** it contains a decision tree: "Confirmed requires high confidence AND zero critical findings; NeedsFix when any major+ finding; FalsePositive when DoneClaim contradicts diff; NeedsHumanReview when low confidence"

### Requirement: Continuation prompt seed includes budget + retrospective
The continuation prompt seed SHALL include: (a) iterations used / max, (b) cost used / max, (c) no-progress counter / threshold, (d) summary of what prior iterations attempted (retrieved from Cold Store or heartbeat highlights), (e) the specific failing findings from the most recent Audit. The Task entering iteration N SHALL know what iterations 1..N-1 tried and why they failed.

#### Scenario: Budget visibility in seed
- **WHEN** the ContinuationCoordinator constructs the prompt seed for iteration 3 of a chain with budget (10, 5.0 USD)
- **THEN** the seed contains "Budget: 2/10 iterations used, $1.20/$5.00 spent"

#### Scenario: Retrospective in seed
- **WHEN** the ContinuationCoordinator constructs the prompt seed for iteration 3
- **THEN** the seed contains a summary of what iterations 1 and 2 attempted and why Audit rejected them
- **AND** the seed instructs the model: "Do not repeat these failed approaches"

### Requirement: Focus prompt includes reporting cadence
The Focus Contract prompt section SHALL specify when to call `report_highlight` (e.g., on finding discovery, on phase transition, on evidence gate failure) — not just "you must monitor these dimensions." Passive monitoring language is replaced with active reporting triggers.

#### Scenario: Reporting triggers in focus prompt
- **WHEN** the focus prompt is rendered for a Task with dimensions `["compilation", "tests", "diff-sanity"]`
- **THEN** the prompt specifies: "Call report_highlight(compilation, ...) immediately when a build fails; call report_highlight(tests, ...) when a test fails; call report_highlight(diff-sanity, ...) when you delete a file"
