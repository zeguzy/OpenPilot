## 1. Project Bootstrap

- [x] 1.1 Initialize Cargo workspace (core lib crate + cli binary crate + test-utils crate)
- [x] 1.2 Add base dependencies (tokio, serde, serde_json, tracing, anyhow, thiserror)
- [x] 1.3 Add test dependencies (rstest, proptest, insta, mockall, tempfile, wiremock)
- [x] 1.4 Configure clippy (pedantic + nursery) and rustfmt
- [x] 1.5 Set up CI skeleton (cargo test + cargo clippy + cargo fmt --check)

## 2. TDD Foundation (tdd-foundation spec)

- [x] 2.1 Define DI traits: FileSystem, Process, Clock, Random
- [x] 2.2 Implement MockFileSystem (in-memory), MockProcess, FakeClock, FakeRandom
- [x] 2.3 Write tests proving DI traits are injectable and deterministic
- [x] 2.4 Set up insta snapshot test conventions (.snap file location, review workflow)

## 3. Memory System (memory-system spec, Phase 1 TDD)

- [x] 3.1 Define Memory<T> generic component (active: Vec<T>, archive: Store, index: MultiIndex)
- [x] 3.2 Implement Store with in-memory SQLite backend (keyword inverted index)
- [x] 3.3 Write TDD tests: remember stores with tags, recall by keyword returns correct items
- [x] 3.4 Implement compact operation (threshold detection, summary generation, archive move)
- [x] 3.5 Write TDD tests: compact reduces active token count, no data loss (proptest)
- [x] 3.6 Add time-based index (recall by time range)
- [x] 3.7 Add task_id index (recall by task_id)
- [x] 3.8 Add tag index (recall by focus tag)
- [x] 3.9 Write integration tests for multi-dimensional recall queries
- [x] 3.10 Implement Orchestrator-specific compaction strategy (completed task highlights → 1 summary, old heartbeats → discard from active)

## 4. Lifecycle State Machine (task-lifecycle spec, Phase 1 TDD)

- [x] 4.1 Define TaskStatus enum (Sleeping, Waking, Pondering, OnIt, Waiting, Reviewing, Delivered, Stuck, Axed, Archived)
- [x] 4.2 Define valid transition table
- [x] 4.3 Write TDD tests: all valid transitions succeed
- [x] 4.4 Write TDD tests: invalid transitions rejected (proptest for arbitrary sequences)
- [x] 4.5 Implement heartbeat auto-push on state transition
- [x] 4.6 Write snapshot tests for heartbeat JSON format
- [x] 4.7 Implement panic-to-Crashed handling (JoinHandle panic catch → terminal error state)

## 5. Workspace Isolation (workspace-isolation spec, Phase 1 TDD)

- [x] 5.1 Define Workspace trait (create, path, freeze, diff, merge_into, cleanup) and ChangeSet/MergeResult types
- [x] 5.2 Implement CopyWorkspace (full directory copy, baseline via tempfile)
- [x] 5.3 Write TDD tests: CopyWorkspace create → write → diff → merge → cleanup
- [x] 5.4 Implement GitWorkspace (git worktree add/remove via git2 or subprocess)
- [x] 5.5 Write TDD tests: GitWorkspace isolation (two workspaces don't affect each other)
- [x] 5.6 Write TDD tests: GitWorkspace diff against baseline
- [x] 5.7 Write TDD tests: GitWorkspace merge (clean + conflict detection)
- [x] 5.8 Implement MirrorWorkspace (internal git repo in .agent/mirror/, import + worktree)
- [x] 5.9 Write TDD tests: MirrorWorkspace for non-git project (create, diff, patch-apply merge)
- [x] 5.10 Implement .agentignore parsing + symlink for excluded dirs (node_modules, target)
- [x] 5.11 Implement CoW detection (clonefile on macOS, reflink on Linux) with full-copy fallback
- [x] 5.12 Implement workspace cleanup with configurable delay (default 3 days)
- [x] 5.13 Implement WorkspaceManager (auto-detect type, create appropriate implementation)

## 6. Provider Abstraction (provider-abstraction spec, Phase 1-2 TDD)

- [x] 6.1 Define Provider trait (async fn stream → Stream<Event>) and Event/Message/ToolDef types
- [x] 6.2 Define ToolEffects enum (read/write/append/process) for parallel classification
- [x] 6.3 Implement ScriptedProvider (then_tool_call, then_text, then_tool_result, then_done chaining)
- [x] 6.4 Write TDD tests: ScriptedProvider drives a mock agent loop deterministically
- [x] 6.5 Write TDD tests: ScriptedProvider exhausted → clear error
- [x] 6.6 Implement zero-copy context builder (Cow<'_, [Message]>, cached tool defs)
- [x] 6.7 Write benchmarks: context build with 200 messages (zero-copy vs clone comparison)

## 7. Task Context Layering & Focus Contract (task-context-layering spec, Phase 1-2 TDD)

- [x] 7.1 Define FocusContract struct (dimensions: Vec<String>, cap: 8)
- [x] 7.2 Write TDD tests: add/remove dimensions, cap enforcement at 8
- [x] 7.3 Write TDD tests: proptest for arbitrary add/remove sequences within cap
- [x] 7.4 Define Highlight struct (tag, severity, summary, detail) and report_highlight tool def
- [x] 7.5 Write TDD tests: highlight tag must match focus dimension, else rejected
- [x] 7.6 Implement FocusContract injection into system prompt (template generation)
- [x] 7.7 Write snapshot tests for focus-injected system prompt format
- [x] 7.8 Implement update_focus steering message handling (add/remove on running Task)

## 8. Tool Registry & Built-in Tools (Phase 2 TDD)

- [x] 8.1 Define Tool trait (name, description, schema, effects, execute) and ToolRegistry
- [x] 8.2 Implement ToolEffects-based parallel/serial dispatch (read effects → parallel batch, write effects → serial)
- [x] 8.3 Write TDD tests: parallel dispatch of read-only tools, serial dispatch of write tools
- [x] 8.4 Implement built-in tools: read, write, edit (with ToolEffects classification)
- [x] 8.5 Implement built-in tools: bash, grep, find, ls
- [x] 8.6 Implement report_highlight tool (pushes to Layer 2 + notifies Orchestrator)
- [x] 8.7 Write TDD tests: each tool with ScriptedProvider + MockWorkspace

## 9. Task Agent Loop (Phase 2 TDD)

- [x] 9.1 Implement Task struct (Provider, Workspace, Memory<Message>, FocusContract, ToolRegistry, channels)
- [x] 9.2 Implement steering + follow-up dual queues (steering for in-flight interrupts, follow-up for idle)
- [x] 9.3 Implement agent run loop: receive input → build context (Cow zero-copy) → stream → dispatch tools → loop
- [x] 9.4 Write TDD tests: single-turn with ScriptedProvider (text response)
- [x] 9.5 Write TDD tests: multi-turn with tool calls (read → text → done)
- [x] 9.6 Write TDD tests: steering injects new instruction mid-loop
- [x] 9.7 Write TDD tests: report_highlight pushes to Orchestrator via channel
- [x] 9.8 Write TDD tests: Task transitions through lifecycle states during loop execution
- [x] 9.9 Write TDD tests: heartbeat pushed on each turn completion and state change

## 10. Orchestrator (orchestrator-core spec, Phase 2 TDD)

- [x] 10.1 Implement Orchestrator struct (Active Memory, Archive, Task Registry, Provider, channels)
- [x] 10.2 Implement user message routing (foreground direct reply vs background dispatch)
- [x] 10.3 Write TDD tests: quick question → foreground, long task → background dispatch
- [x] 10.4 Implement Task dispatch with Focus Contract signing
- [x] 10.5 Write TDD tests: dispatched Task receives correct focus dimensions
- [x] 10.6 Implement heartbeat aggregation into Orchestrator Active Memory
- [x] 10.7 Write TDD tests: heartbeat updates Task registry, user query returns latest heartbeat
- [x] 10.8 Implement deep_dive tool (read-only snapshot from Task Layer 3)
- [x] 10.9 Implement recall tool (query Archive/Cold Store)
- [x] 10.10 Implement background async prefetch (keyword match after user message)
- [x] 10.11 Write TDD tests: recall returns correct items by keyword/time/task_id/tag
- [x] 10.12 Implement conflict prediction (file overlap estimation before dispatch)
- [x] 10.13 Write TDD tests: overlapping tasks serialized, non-overlapping parallelized
- [x] 10.14 Implement update_focus (dynamic Focus Contract adjustment via steering)

## 11. Audit Agent (audit-agent spec, Phase 2 TDD)

- [x] 11.1 Implement AuditAgent as Task variant (role=Auditor, readonly=true, simplified lifecycle)
- [x] 11.2 Define Audit focus inheritance (Task focus + standard checks + Orchestrator extras)
- [x] 11.3 Write TDD tests: Audit focus correctly inherited from Task
- [x] 11.4 Implement black-box audit (read diff, run tests/compile, assess)
- [x] 11.5 Implement deep_dive_task_context tool for Audit (read Task full context fragments)
- [x] 11.6 Write TDD tests: black-box pass/warn/fail verdicts with ScriptedProvider
- [x] 11.7 Write TDD tests: deep dive triggered when diff is suspicious
- [x] 11.8 Implement structured Audit report (verdict, confidence, findings, summary)
- [x] 11.9 Write snapshot tests for Audit report format
- [x] 11.10 Implement Orchestrator override of Audit verdict
- [x] 11.11 Write TDD tests: Orchestrator overrides fail → accept with recalled context

## 12. Completion Pipeline (completion-pipeline spec, Phase 2 TDD)

- [x] 12.1 Implement 5-stage pipeline coordinator (Freeze → Review → Merge → Memorialize → Cleanup)
- [x] 12.2 Implement Freeze stage (workspace read-only, final summary generation, heartbeat)
- [x] 12.3 Implement risk assessment (diff size, file types, task type → low/medium/high)
- [x] 12.4 Write TDD tests: low-risk → rule checks only, high-risk → Audit dispatched
- [x] 12.5 Implement Merge stage (conflict detection, Orchestrator auto-resolve attempt)
- [x] 12.6 Write TDD tests: clean merge, auto-resolvable conflict, unresolvable → user notified
- [x] 12.7 Implement Memorialize stage (final summary → Active, full context → Cold Store with indices)
- [x] 12.8 Write TDD tests: Cold Store recallable after Memorialize
- [x] 12.9 Implement Cleanup stage (delayed workspace removal, Cold Store preserved)
- [x] 12.10 Implement dependency chain auto-activation (predecessor merge → successor activated with fresh workspace)
- [x] 12.11 Implement completion notification with user-activity awareness (low-risk silent, high-risk queued)
- [x] 12.12 Write TDD tests: dependency chain auto-activates successor after merge

## 13. Extension System (extension-system spec, Phase 2 TDD)

- [x] 13.1 Implement AGENTS.md loader (project root + upward traversal, @import syntax)
- [x] 13.2 Implement skills/*.md loader (relevance matching, system prompt injection)
- [x] 13.3 Write TDD tests: AGENTS.md injected at session start, skills loaded by relevance
- [x] 13.4 Define Hook system types (HookEvent, HookHandler, HookConfig, Matcher)
- [x] 13.5 Implement 5 handler types: command, http, mcp_tool, prompt, agent
- [x] 13.6 Implement event dispatch (Session/Orchestrator/Task/Audit four levels)
- [x] 13.7 Write TDD tests: on_pre_tool_use hook deny blocks tool call
- [x] 13.8 Write TDD tests: on_merge_pre hook blocks merge on test failure
- [x] 13.9 Write TDD tests: prompt-type hook (LLM single-turn judgment)
- [x] 13.10 Implement MCP client (JSON-RPC over stdin/stdout, tool/resource/prompt discovery)
- [x] 13.11 Write TDD tests: MCP tool appears in registry, MCP crash isolation
- [x] 13.12 Implement plugin.toml manifest parsing and plugin installation
- [x] 13.13 Write TDD tests: plugin install registers context + MCP + hooks
- [x] 13.14 Implement per-Task tool activation (plugin keyword/task_type matching + Orchestrator decision)
- [x] 13.15 Write TDD tests: docker plugin activated for container task, not for auth refactor

## 14. CLI Frontend (cli-frontend spec, Phase 2-3)

- [x] 14.1 Implement non-blocking input loop (tokio task reading stdin, never blocks on Task)
- [x] 14.2 Implement silent background mode (no Task output by default)
- [x] 14.3 Implement completion notification (🔔 line on Task Delivered)
- [x] 14.4 Implement progress query ("how is task A going?" → Orchestrator heartbeat)
- [x] 14.5 Implement "what's running?" command (list all active Tasks)
- [x] 14.6 Implement pending review indicator (status line without popup)
- [x] 14.7 Implement accept/reject commands for completed Tasks
- [x] 14.8 Implement line editing (reedline or rustyline integration)
- [x] 14.9 Write integration tests: input while Task running, notification on completion

## 15. Session Persistence (Phase 2-3)

- [x] 15.1 Define session format (JSONL primary + SQLite index, dual-layer like pi_agent_rust)
- [x] 15.2 Implement session save (Orchestrator conversation + Task states)
- [x] 15.3 Implement session restore (reload conversation + active Tasks from JSONL)
- [x] 15.4 Write TDD tests: save → restore roundtrip preserves all state
- [x] 15.5 Implement Cold Store persistence (cross-session recall)

## 16. Real Provider Implementations (Phase 3, E2E)

- [x] 16.1 Implement AnthropicProvider (reqwest + SSE streaming, tool calling, system prompt)
- [x] 16.2 Write E2E smoke test: real Anthropic API, simple conversation
- [x] 16.3 Implement OpenAIProvider (reqwest + SSE, function calling)
- [x] 16.4 Write E2E smoke test: real OpenAI API
- [x] 16.5 Implement GeminiProvider (reqwest + SSE)
- [x] 16.6 Write E2E smoke test: real Gemini API

## 17. E2E Integration (Phase 3)

- [x] 17.1 E2E: dispatch background Task in real git project → worktree → agent loop → completion → merge
- [x] 17.2 E2E: dispatch Task in non-git project → mirror → agent loop → patch merge
- [x] 17.3 E2E: two parallel Tasks with non-overlapping files → both complete → sequential merge
- [x] 17.4 E2E: Task triggers Audit → Audit verdict → Orchestrator decision → accept/reject
- [x] 17.5 E2E: focus contract dynamic update mid-Task → Task reports new dimension
- [x] 17.6 E2E: Orchestrator recall retrieves info from previous session (Cold Store)
- [x] 17.7 E2E: MCP server provides tool → Task uses it → result integrated
- [x] 17.8 E2E: plugin install → context + MCP + hooks all active in session
- [x] 17.9 E2E: hook blocks dangerous bash command (on_pre_tool_use deny)
- [x] 17.10 E2E: Task panic → Crashed state → notification → workspace recoverable

## 18. Polish & Documentation

- [x] 18.1 Write CLI help text and onboarding (--help, first-run guide)
- [x] 18.2 Write configuration documentation (.agent/config.toml, .agentignore)
- [x] 18.3 Write plugin authoring guide (plugin.toml format, skill/skill/hook conventions)
- [x] 18.4 Write AGENTS.md for the project itself (dogfooding)
- [x] 18.5 Performance benchmarks: workspace creation (CoW vs copy), context building (zero-copy), Memory recall
- [x] 18.6 Audit codebase for clippy warnings, dead code, missing tests
