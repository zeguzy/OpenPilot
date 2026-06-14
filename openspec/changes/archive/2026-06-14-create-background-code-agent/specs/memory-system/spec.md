## ADDED Requirements

### Requirement: Memory component with active and archive
The system SHALL provide a generic `Memory<T>` component with an active region (current context window) and an archive region (persistent storage with index). The same component SHALL be reusable across Task, Orchestrator, and cross-session scopes (fractal pattern).

#### Scenario: Task uses Memory for messages
- **WHEN** Task A processes a turn
- **THEN** messages are stored in `Memory<Message>` active region

#### Scenario: Orchestrator uses Memory for conversation events
- **WHEN** Orchestrator receives a heartbeat and a user message
- **THEN** both are stored in `Memory<ConversationEvent>` active region

### Requirement: Memory compact moves old content to archive
When the active region approaches the context window limit, the system SHALL compact: compress older active content into a summary, store the full content in the archive with indices, and keep only recent items plus the summary in active.

#### Scenario: Compaction reduces active token count
- **WHEN** active region reaches 80% of the token limit
- **THEN** older items are compressed into a summary
- **AND** the full content is stored in the archive
- **AND** active token count drops below 50% of the limit

#### Scenario: Compaction preserves data integrity
- **WHEN** compaction runs on Memory with 100 items
- **THEN** all 100 items are retrievable from the archive via recall
- **AND** no data is lost

### Requirement: Memory recall with multi-dimensional index
The archive SHALL support recall via multiple index dimensions: keyword (inverted index), time range, task_id, tag (focus tags), and semantic (embedding similarity).

#### Scenario: Recall by keyword
- **WHEN** recall(query="auth refactor") is called
- **THEN** archive items containing "auth" or "refactor" keywords are returned

#### Scenario: Recall by task_id
- **WHEN** recall(task_id="task-A-abc123") is called
- **THEN** all archive items associated with Task A are returned

#### Scenario: Recall by tag
- **WHEN** recall(tag="security risks") is called
- **THEN** all highlights and summaries tagged with "security risks" are returned

### Requirement: Memory remember stores with tags
The system SHALL provide a `remember(item, tags)` operation that stores an item in the archive and builds indices for all specified tags plus auto-extracted keywords.

#### Scenario: Remember with tags
- **WHEN** remember(item=summary, tags=["auth", "oauth2", "task-A-abc123"]) is called
- **THEN** the item is stored in archive
- **AND** it is retrievable via recall by any of the tags

### Requirement: Orchestrator compaction strategy
The Orchestrator's Memory SHALL use task-aware compaction: completed Task highlights are compressed into a single final summary, in-progress Task old highlights are rolling-compressed, and only the latest heartbeat per Task is retained.

#### Scenario: Completed task highlights compressed
- **WHEN** Task A (completed) had 5 highlights in Orchestrator's active memory
- **THEN** after compaction, 5 highlights are replaced by 1 final summary
- **AND** all 5 original highlights remain in archive

#### Scenario: Old heartbeats discarded
- **WHEN** Task A pushed 10 heartbeats over its lifetime
- **THEN** after compaction, only the latest heartbeat is retained in active memory
- **AND** older heartbeats are in archive

### Requirement: Recall triggered by Orchestrator on-demand
The Orchestrator SHALL decide autonomously whether to recall (via LLM judgment), with a background async prefetch that runs keyword matching after each user message and injects relevant summaries if found.

#### Scenario: LLM decides to recall
- **WHEN** user asks "what did we find last time about auth?"
- **THEN** Orchestrator calls recall(query="auth") and injects results into context

#### Scenario: Background prefetch without latency impact
- **WHEN** user sends a message
- **THEN** a background prefetch runs asynchronously without blocking the first response
- **AND** if hits are found, summaries are injected into the next turn's context
