## ADDED Requirements

### Requirement: Task enforces Evidence Gate before Delivered
Before transitioning from `OnIt` to `Delivered`, the Task SHALL execute the project's configured evidence commands (default: `cargo build`, `cargo test --no-run`, `cargo clippy --workspace --all-targets`). If any command fails, the Task SHALL NOT transition to `Delivered`; instead it enters a 3-strike retry loop. Pre-existing failures (failures present on the baseline workspace before the Task's changes) are detected and excluded from the gate.

#### Scenario: Evidence passes
- **WHEN** Task A's model emits a final summary without further tool calls
- **AND** `cargo build`, `cargo test --no-run`, and `cargo clippy` all succeed
- **THEN** Task A transitions to `Delivered`

#### Scenario: Evidence fails on first attempt
- **WHEN** Task A's model emits a final summary
- **AND** `cargo build` fails with a type error
- **THEN** Task A does NOT transition to `Delivered`
- **AND** Task A re-enters the execution loop with the build error in context
- **AND** a Layer 2 highlight is emitted with tag `evidence-gate` severity `major`

#### Scenario: Pre-existing failure excluded
- **WHEN** Task A's changes are in `src/auth.rs` and `cargo test` fails on a pre-existing test in `src/legacy.rs` that was already broken before Task A's changes
- **THEN** the gate considers only failures introduced by Task A's diff
- **AND** Task A may transition to `Delivered` if its own changes are clean

#### Scenario: Custom evidence commands
- **WHEN** `.agent/config.toml` has `[task] evidence_commands = ["npm test", "npm run lint"]`
- **THEN** the Task runs those commands instead of the cargo defaults

### Requirement: Task can pause for user clarification via Waiting
When a Task encounters ambiguity that cannot be resolved from context (Phase 0 Intent Gate fails), the Task SHALL transition to `Waiting` state with a structured clarification request. The Task's run loop suspends until either (a) the Orchestrator forwards a steering message with the user's answer, or (b) a timeout expires (default 5 minutes) and the Task proceeds with its best-guess interpretation.

#### Scenario: Task pauses for clarification
- **WHEN** Task A encounters an ambiguous requirement ("refactor the auth module" with no specific goal)
- **THEN** Task A transitions to `Waiting` with a clarification question in its heartbeat
- **AND** the Orchestrator surfaces the question to the user via a structured notification

#### Scenario: User responds via steering
- **WHEN** the user answers Task A's clarification via the Orchestrator
- **THEN** the Orchestrator forwards the answer as a `SteeringMessage::Inject`
- **AND** Task A transitions back to `OnIt` with the answer in context

#### Scenario: Clarification timeout
- **WHEN** Task A has been in `Waiting` for 5 minutes without a response
- **THEN** Task A transitions back to `OnIt` and proceeds with its best-guess interpretation
- **AND** a Layer 2 highlight notes the timeout and the chosen interpretation

### Requirement: Task enforces 3-strike failure rule
When a Task fails to fix the same issue across 3 consecutive attempts (identified by issue signature — same file, same error pattern), the Task SHALL transition to `Stuck` rather than retrying indefinitely. The Task's heartbeat SHALL include the failure context so the Orchestrator can surface it for steering.

#### Scenario: 3 consecutive failures at same issue
- **WHEN** Task A attempts to fix a type error in `src/lib.rs:42`, fails, attempts again with a different approach, fails, attempts a third time, fails
- **THEN** Task A transitions to `Stuck` with reason "3-strike: type error in src/lib.rs:42"

#### Scenario: Different issue resets counter
- **WHEN** Task A fails twice at issue X, then encounters different issue Y
- **THEN** the counter for issue X is preserved but a new counter starts for issue Y

#### Scenario: Stuck Task can be steered
- **WHEN** Task A is `Stuck` and the Orchestrator forwards a `SteeringMessage::Inject` with new context
- **THEN** Task A transitions back to `OnIt` and the 3-strike counter for the prior issue resets

### Requirement: Task creates TodoWrite list for multi-step work
When a Task identifies that its work involves 3 or more distinct steps, the Task SHALL create a todo list via the `TodoWrite` tool before starting execution. The todo list is included in the Task's Layer 1 heartbeat so the Orchestrator can surface real-time progress. As steps complete, the Task updates the todo list.

#### Scenario: Todo created at start
- **WHEN** Task A receives a request to "implement registration, catalog, cart, checkout"
- **THEN** Task A's first action is to call `TodoWrite` with 4 items
- **AND** the first item is marked `in_progress`

#### Scenario: Progress surfaces in heartbeat
- **WHEN** Task A completes item 1 of 4 and starts item 2
- **THEN** Task A's Layer 1 heartbeat shows: `todo: {total: 4, completed: 1, in_progress: "item 2"}`

#### Scenario: Trivial work skips todo
- **WHEN** Task A receives a single-step request ("rename variable X to Y")
- **THEN** Task A does NOT create a todo list
- **AND** the heartbeat's `todo` field is empty
