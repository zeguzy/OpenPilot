## ADDED Requirements

### Requirement: Context extension via Markdown files
The system SHALL support context injection via AGENTS.md files (project-level) and skills/*.md files (capability-specific). These are pure Markdown injected into the system prompt—no code execution.

#### Scenario: AGENTS.md injected at session start
- **WHEN** a session starts in a project with AGENTS.md
- **THEN** the AGENTS.md content is injected into the Orchestrator's system prompt

#### Scenario: Skill loaded by relevance
- **WHEN** a Task is dispatched for "refactor Rust code"
- **THEN** skills matching "rust" or "refactor" relevance are loaded into the Task's system prompt

### Requirement: Capability extension via MCP servers
The system SHALL support MCP (Model Context Protocol) servers as external processes providing tools, resources, and prompts. MCP tools SHALL appear alongside built-in tools in the Agent's tool registry.

#### Scenario: MCP tool available to Agent
- **WHEN** an MCP server providing "github_create_issue" is configured
- **THEN** the tool appears in the Agent's tool list as `mcp__github__create_issue`
- **AND** the Agent can call it like any built-in tool

#### Scenario: MCP server crash isolation
- **WHEN** an MCP server process crashes
- **THEN** the main agent process is unaffected
- **AND** the tool is marked unavailable with an error message

### Requirement: Hook system with lifecycle events
The system SHALL provide lifecycle hooks across four levels: Session (on_session_start, on_session_end), Orchestrator (on_user_message, on_pre_dispatch, on_merge_pre, on_merge_post, etc.), Task (on_task_create, on_task_freeze, on_pre_tool_use, on_post_tool_use, etc.), and Audit (on_audit_start, on_audit_report, on_audit_override).

#### Scenario: Pre-tool-use hook blocks dangerous command
- **WHEN** on_pre_tool_use hook for Bash receives `rm -rf /` and returns deny
- **THEN** the tool call is blocked and the Agent receives an error message

#### Scenario: Merge-pre hook runs tests
- **WHEN** on_merge_pre hook is configured to run "cargo test"
- **THEN** before any merge, cargo test runs
- **AND** if tests fail, the merge is blocked

### Requirement: Hooks support five handler types
Each hook SHALL support five handler types: command (shell script), http (POST JSON), mcp_tool (call MCP tool), prompt (LLM single-turn judgment), agent (spawn subagent to verify). The prompt and agent types enable LLM-as-hook-judge.

#### Scenario: Prompt-type hook for safety check
- **WHEN** on_pre_tool_use hook of type "prompt" receives a bash command
- **THEN** a single-turn LLM call evaluates "is this command safe?"
- **AND** the LLM's yes/no decision determines allow/deny

### Requirement: Plugin is a packaging format
A Plugin SHALL be a directory containing a plugin.toml manifest that bundles Context (AGENTS.md, skills/), Capability (mcp.json), and Hook (hooks.toml) components. Plugins do not introduce new extension mechanisms.

#### Scenario: Plugin installation
- **WHEN** `agent plugin install ./my-plugin` is run
- **THEN** the plugin's AGENTS.md is registered as context
- **AND** the plugin's mcp.json servers are started
- **AND** the plugin's hooks.toml hooks are registered to events

### Requirement: Tool activation is per-Task selective
The Orchestrator SHALL selectively activate plugin tools per Task based on plugin declarations (keywords, task_types) and Orchestrator judgment. Not all installed plugins' tools are available to every Task.

#### Scenario: Docker plugin activated only for container tasks
- **WHEN** Task A is "refactor auth" (no container relevance)
- **THEN** docker plugin tools are NOT activated for Task A
- **AND** Task A's tool list does not include docker tools

#### Scenario: Docker plugin activated for deployment task
- **WHEN** Task B is "containerize the app"
- **THEN** docker plugin tools ARE activated for Task B based on keyword match

### Requirement: Provider plugins via HTTP proxy
The system SHALL allow plugins to provide new LLM providers via a local HTTP server proxy pattern. The framework communicates with the provider plugin via HTTP/SSE, enabling streaming.

#### Scenario: Local Ollama provider via plugin
- **WHEN** a provider plugin exposes a local HTTP server proxying Ollama
- **THEN** the framework can use Ollama as a Provider via HTTP/SSE streaming
