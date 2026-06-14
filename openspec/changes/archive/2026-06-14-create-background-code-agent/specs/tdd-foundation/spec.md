## ADDED Requirements

### Requirement: Dependency injection via traits
All external dependencies (filesystem, subprocess, clock, random) SHALL be abstracted behind traits and injected. No direct use of `std::fs`, `std::process::Command`, `SystemTime`, or `rand` in core logic. This enables deterministic testing.

#### Scenario: FileSystem trait replaces std::fs
- **WHEN** core logic needs to read a file
- **THEN** it calls `fs.read(path)` via a `FileSystem` trait
- **AND** tests inject a MockFileSystem that returns controlled content

#### Scenario: Clock trait for deterministic time
- **WHEN** core logic needs the current time
- **THEN** it calls `clock.now()` via a `Clock` trait
- **AND** tests inject a FakeClock with controllable time advancement

### Requirement: ScriptedProvider as test infrastructure
The system SHALL provide ScriptedProvider as a first-class test utility, enabling fully deterministic agent loop tests without any LLM API calls.

#### Scenario: Full agent loop test without LLM
- **WHEN** a test creates a Task with ScriptedProvider and MockWorkspace
- **THEN** the entire agent loop runs deterministically
- **AND** the test asserts on state transitions, heartbeats, and highlights

### Requirement: MockWorkspace for isolated workspace tests
The system SHALL provide a MockWorkspace implementing the Workspace trait that operates in-memory or via tempfile, enabling fast workspace tests without real git operations.

#### Scenario: MockWorkspace diff
- **WHEN** a test writes to MockWorkspace and calls diff()
- **THEN** a ChangeSet is returned without any real git operations

### Requirement: Snapshot testing for structured outputs
The system SHALL use snapshot testing (insta) for structured outputs like heartbeat format, highlight structure, prompt templates, and diff output format. Snapshots are reviewed and committed to git.

#### Scenario: Heartbeat format snapshot
- **WHEN** a heartbeat is formatted
- **THEN** insta::assert_json_snapshot captures the format
- **AND** format changes are caught by snapshot diff in CI

### Requirement: Property testing for state machine and memory
The system SHALL use property-based testing (proptest) for: lifecycle state machine transition legality, Memory compact data integrity, and Focus Contract cap enforcement under arbitrary add/remove sequences.

#### Scenario: State machine property test
- **WHEN** proptest generates random state transition sequences
- **THEN** only valid transitions are accepted and invalid ones rejected
- **AND** no sequence causes a panic or inconsistent state

#### Scenario: Memory compact never loses data
- **WHEN** proptest generates random Memory operations (remember + compact)
- **THEN** all remembered items are retrievable from archive via recall
- **AND** no data is lost regardless of operation order

### Requirement: Three-layer test strategy
The test suite SHALL follow a three-layer strategy: Layer 1 unit tests (60-70%, pure logic, standard TDD), Layer 2 integration tests (20%, ScriptedProvider + MockWorkspace, multi-module), Layer 3 E2E smoke tests (5-10%, real LLM, real git, CI only).

#### Scenario: Layer 1 unit test for Memory
- **WHEN** testing Memory compact
- **THEN** a pure unit test with in-memory SQLite runs in milliseconds
- **AND** no external dependencies (no LLM, no git, no network)

#### Scenario: Layer 2 integration test for Task lifecycle
- **WHEN** testing Task lifecycle with heartbeats
- **THEN** ScriptedProvider drives the agent loop
- **AND** heartbeat/highlight assertions verify state transitions
