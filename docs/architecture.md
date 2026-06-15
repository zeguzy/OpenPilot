[English](architecture.md) | [中文](architecture.zh-CN.md)

# Architecture

This document describes how opca fits together. It is the engineering
reference behind the [README](../README.md) pitch: the process model,
the three roles, the state machines, and the design decisions that
shaped each subsystem.

For setup and daily use, read [getting-started.md](getting-started.md).
For runtime knobs, read [configuration.md](configuration.md).

## Overview

opca runs three roles inside a single process.

```
┌─ single process, tokio runtime ────────────────────────────┐
│                                                            │
│  CLI foreground (tokio task)                               │
│    input loop: always reading stdin                        │
│    output: Orchestrator replies + Task completion notices  │
│         ↕ mpsc channel                                     │
│  Orchestrator (tokio task)                                 │
│    Active Memory (context window)                          │
│    Archive + Cold Store (SQLite indices)                   │
│    Task Registry (state of every active Task)              │
│    Tools: recall, deep_dive, dispatch, update_focus        │
│         ↕ per-task channel pair (steering + heartbeat)     │
│  Task A (tokio::spawn)        Task B (tokio::spawn)        │
│    Agent Loop                   Agent Loop                 │
│    Memory<Message>             Memory<Message>             │
│    Workspace (worktree)        Workspace (worktree)        │
│    Tool Registry               Tool Registry               │
│         ↕                                                  │
│  Audit (tokio::spawn, on demand)                           │
│    read-only diff + test runs + verdict                    │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

- **Orchestrator.** One per session. The only thing the user talks to.
  Routes messages (quick reply vs. background work), dispatches Tasks,
  aggregates heartbeats, manages Focus Contracts, recalls from the cold
  store, and decides whether each finished Task merges, needs audit, or
  asks the user.
- **Task.** A background worker. Owns its own provider, workspace,
  `Memory<Message>`, focus contract, and tool registry. Reports up
  through three layers: an automatic heartbeat, on-demand highlights,
  and a full message history the Orchestrator pulls on demand.
- **Audit.** A read-only specialization of Task (`role=Auditor,
  readonly=true`). Spawn, inspect, judge, report, die. Defaults to a
  cheap model for cost control. The Orchestrator can override its
  verdict.

The non-blocking experience comes from this split. The Orchestrator
owns the conversation. Tasks own the work. Channels connect them. When
you dispatch a slow job, the Orchestrator hands it off and stays
responsive to your next message.

## Process model

opca is one process. The Orchestrator and every Task are tokio tasks
spawned with `tokio::spawn`, communicating through mpsc channels.

This decision came down to three things. Tasks need to share state
(the workspace manager, the cold store, the task registry), and
in-process sharing beats IPC for that. Channel latency is negligible
inside one process. And task crashes are handled by awaiting the
`JoinHandle`, catching the panic, flipping the Task to a crashed state,
and notifying the Orchestrator. Process isolation is not worth the
overhead.

The trade-off is that one process holds everything. If the binary
itself dies, all Tasks die with it. That is acceptable for a local
single-user tool, and it keeps deployment to a single binary.

The foreground CLI is also a tokio task. Its input loop is always
reading stdin. Its output side prints Orchestrator replies and Task
completion notices. Background Tasks default to silent output: you see
a heartbeat on dispatch, a notification on completion, and nothing in
between unless you ask.

## Orchestrator

The Orchestrator is the main brain. It holds the active memory, the
archive, the cold store, the task registry, and the routing logic.

### Routing

Every incoming message gets classified. Quick questions ("what does X
do?", "remind me about Y") get an instant reply from the Orchestrator's
own context. Anything that looks like real work ("refactor X", "add
tests for Y") gets dispatched to a background Task. The Orchestrator
also handles natural-language status queries by pulling from the task
registry.

### Task dispatch and Focus Contract

When the Orchestrator dispatches a Task, it signs a Focus Contract.
This is the list of dimensions the Task must report on: security risks,
breaking changes, decisions needing confirmation, blockers, and so on.
The contract gets injected into the Task's system prompt, and the
Task's `report_highlight` tool requires a `tag` that matches the focus
list.

The contract is dynamic. The Orchestrator can send `update_focus` over
the steering channel to add or remove dimensions while the Task runs.
There is a hard cap of eight active dimensions. To add a ninth, the
Orchestrator must first remove one. This keeps the Orchestrator's
context from drowning in highlight noise.

### Heartbeat aggregation and deep_dive

The Orchestrator sees each Task through three layers, and by default it
only watches two of them.

Layer 1 is the heartbeat. Every Task pushes one automatically at the end
of each turn and on every state transition. It is around 50 tokens:
status, progress, what the Task is doing right now. The Orchestrator
aggregates these into its task registry.

Layer 2 is highlights. A Task pushes a highlight when it calls
`report_highlight`, tagged with a focus dimension. These are 100 to 300
tokens each. They enter the Orchestrator's active context, so the
Orchestrator can mention them naturally when replying to the user.

Layer 3 is the full message history. It never gets pushed. The
Orchestrator pulls fragments of it on demand through the `deep_dive`
tool, when it needs to understand a Task's reasoning in detail.

This layering is what lets the Orchestrator manage many Tasks at once
without blowing its context window. It sees the shape of every Task
through layers 1 and 2, and only pays for layer 3 detail when something
demands it.

### Recall

The Orchestrator decides for itself whether to recall. When the user
finishes speaking, a background keyword search runs against the archive.
If it hits, relevant summaries drop into the context without adding to
the first-response latency. The Orchestrator can also call `recall`
explicitly when a question seems to reference past work.

### Conflict prediction

Before dispatch, the Orchestrator does a lightweight prediction of
which files each Task will touch. If two Tasks look likely to overlap,
the Orchestrator serializes them rather than running them in parallel.
This prevents most merge conflicts at dispatch time. When a conflict
slips through anyway, the Orchestrator attempts auto-resolution during
the merge stage, and escalates to the user if that fails.

## Task

A Task is a background worker with its own agent loop, memory,
workspace, focus contract, and tool registry. Several run in parallel.

### Agent run loop

The loop is a standard turn-based agent cycle: build context, call the
provider, execute any tool calls, repeat until the Task declares done or
needs input. Each turn ends by pushing a heartbeat. Tool calls are
classified by their effects (read, write, append, process), and the
framework decides which can run in parallel. Read-class tools run
together. Write-class tools serialize.

### Steering and follow-up queues

Tasks receive external input through two queues.

The **steering queue** is for messages that arrive while the Task is
actively working. An `update_focus` from the Orchestrator lands here,
and the Task picks it up between turns. This is how a Focus Contract
gets adjusted mid-flight.

The **follow-up queue** is for messages that arrive while the Task is
idle, waiting, or done. A user follow-up that should restart a
`Waiting` Task goes here.

This pair is the mechanism behind the non-blocking experience. The Task
never blocks on a synchronous request to the Orchestrator, and the
Orchestrator never blocks waiting for a Task. Everything flows through
channels.

### Three-layer context

Each Task maintains the three context layers described under the
Orchestrator. The heartbeat and highlights flow up. The full history
stays local to the Task, pulled only through `deep_dive`.

### Lifecycle state machine

Every Task moves through the same named states. Each transition pushes
a heartbeat automatically.

```
💤 Sleeping → 🌅 Waking → 🤔 Pondering → 🔨 OnIt
   ↓ on completion          ↓ on input needed
✅ Delivered                🫥 Waiting
   ↓ risk assessment             ↓ on reply / ↓ on timeout
🔍 Reviewing → 📦 Archived       OnIt / ✂️ Axed
😵 Stuck → ✂️ Axed → 📦 Archived
```

The states and their meaning:

| State        | Meaning                                                  |
|--------------|----------------------------------------------------------|
| 💤 Sleeping  | Just spawned. Loading config, plugins, context.          |
| 🌅 Waking    | Initializing the workspace, loading context.             |
| 🤔 Pondering | Thinking through the plan (a planning turn).             |
| 🔨 OnIt      | Executing. Doing the actual work.                        |
| 🫥 Waiting   | Needs input from the user or Orchestrator.               |
| 🔍 Reviewing | Finished self-check, or being audited.                   |
| ✅ Delivered | Done. Result awaiting acceptance.                        |
| 😵 Stuck     | Blocked. Needs help to proceed.                          |
| ✂️ Axed      | Cancelled by the user or Orchestrator.                   |
| 📦 Archived  | Fully archived. Workspace scheduled for cleanup.         |

The legal transitions:

- `Sleeping` to `Waking` on initialization.
- `Waking` to `Pondering` on the first turn.
- `Pondering` to `OnIt` when execution starts.
- `OnIt` to `Pondering` for the next turn, `Waiting` when input is
  needed, `Delivered` on completion, or `Stuck` when blocked.
- `Waiting` to `OnIt` on reply, or `Axed` on timeout.
- `Delivered` to `Reviewing` when audit kicks in, or straight to
  `Archived` when the task is low-risk and skips audit.
- `Reviewing` to `Archived` on acceptance, back to `OnIt` on rejection,
  or `Axed` if discarded.
- `Stuck` to `OnIt` when help arrives, or `Axed` when abandoned.
- Any state to `Axed` on cancellation.
- `Axed` to `Archived` after cleanup.

The personified names are deliberate. They make the task list
scannable at a glance, and they map cleanly onto the work the Task is
actually doing. You can tell from `/tasks` whether something is still
thinking, actively executing, or stuck waiting on you.

## Audit

The Audit agent is a read-only specialization of Task, not a separate
top-level concept. It reuses the Task abstraction, the memory, the
focus contract, and the highlight mechanism. The difference is
configuration: `role=Auditor`, `readonly=true`, and a minimal lifecycle.

### Minimal lifecycle

Audit does not go through Sleeping, Waking, Pondering, and the rest.
Its lifecycle is intentionally short:

```
Spawn → Inspect → Judge → Report → Die
```

### Audit mode

By default, Audit works black-box. It reads the diff, reads the final
summary, and runs the tests. It does not look at the Task's reasoning
chain. If something in the diff looks suspicious, Audit can use
`deep_dive_task_context` to pull fragments of the Task's full history
and inspect the reasoning that led to a change. Audit decides for itself
when to go deep.

### Report structure

The verdict comes back as structured JSON:

```json
{
  "verdict": "pass",
  "confidence": 0.86,
  "findings": [
    {
      "severity": "minor",
      "location": "auth.rs:42",
      "issue": "token refresh misses the clock-skew edge case"
    }
  ],
  "summary": "Solid overall. One boundary condition to revisit in token refresh."
}
```

`verdict` is one of `pass`, `warn`, or `fail`. `confidence` is a score
from 0.0 to 1.0. Each finding carries a severity and a location.

### Power chain and override

The chain of authority is Audit (advisor, inspects and judges) to
Orchestrator (decision maker, can override the verdict) to user (final
say on high-risk work). The Orchestrator is the decision maker. Audit
is an advisor. If Audit returns a verdict the Orchestrator disagrees
with, the Orchestrator can override it. High-risk tasks always escalate
to the user for a final decision.

### Cost control

Audit defaults to a cheap model. The thinking is that a cheap model is
good enough to spot "tests fail" or "this diff touches auth and has no
tests", and cost adds up fast when every completed Task triggers an
audit. For genuinely high-risk work, the Orchestrator escalates to a
stronger model. You can override the audit model per-project through
`model.audit_model` in the config.

## Memory

Memory is fractal. The same `Memory<T>` shape is reused at three levels.

```
Memory<T> {
  active: Vec<T>          // current context window
  archive: Store          // compacted history in SQLite
  index: MultiIndex       // keyword, time, task_id, tag

  compact()               // when the window nears its cap,
                          //   compress old items into the archive
                          //   and index them

  recall(query) -> Vec<T> // search the archive by index
  remember(item, tags)    // write to the archive and index it
}
```

### Three regions

- **Active.** The current context window. What the agent sees right now.
- **Archive.** Compacted history in SQLite, indexed across several
  dimensions so it can be searched without re-reading everything.
- **Cold Store.** Cross-session persistent memory. Anything archived
  here survives across sessions and stays recallable later.

### Fractal reuse

The same component appears at every level.

- **Task** uses `Memory<Message>`. Active is the in-loop context.
  Archive holds the full Layer 3 history.
- **Orchestrator** uses `Memory<ConversationEvent>`. Active is the main
  context. Archive holds compacted history.
- **Cold Store** uses `Memory<SessionSummary>`. Persistent across
  sessions.

Because the shape is identical, the compaction and recall logic is
written once and reused everywhere.

### Index dimensions

The archive is indexed along five dimensions so recall can slice it
several ways.

| Dimension | What it indexes                              |
|-----------|----------------------------------------------|
| keyword   | Inverted index over tokenized content.       |
| time      | Time range queries.                          |
| task_id   | Everything produced by a given Task.         |
| tag       | Focus tags carried over from highlights.     |
| semantic  | Embedding vector retrieval (pluggable model).|

### Compaction

Compaction moves old items out of the active window and into the
archive. The Orchestrator applies specific rules.

Completed Tasks have their highlights compressed into a single final
summary. In-progress Tasks keep their old highlights rolled down as
new ones arrive. Heartbeats only ever keep the latest one per Task. The
active window holds the recent items plus summaries.

The active cap is configurable through `memory.max_active_tokens`.

## Workspace

Each Task works inside an isolated workspace so concurrent Tasks never
step on each other. The `WorkspaceManager` picks the right strategy
automatically.

### Three strategies

**GitWorkspace** is the default for git projects. It runs `git worktree
add` to create a linked working tree. This is the fastest and cleanest
option, and it produces a real git diff for the merge stage.

**MirrorWorkspace** handles non-git projects. It creates a fresh git
repository under `.agent/mirror/`, imports the project's text files,
and checks out a worktree per Task. The merge flow for non-git projects
extracts the worktree diff, applies it as a patch to the original
project directory, and refreshes the mirror baseline.

**CopyWorkspace** is the last-resort fallback. It does a full recursive
directory copy. Highest disk usage, slowest creation, but it works
everywhere with no git dependency.

### Copy-on-Write

Mirror creation uses Copy-on-Write when the filesystem supports it.
`clonefile` on macOS APFS, `reflink` on Linux btrfs and xfs. On other
filesystems, it falls back to a full recursive copy. CoW makes mirror
creation cheap even on large projects, because the data blocks are
shared until written.

### agentignore

A `.agentignore` file (same syntax as `.gitignore`) excludes heavy
directories from mirror imports and copy walks. The common targets are
`target/`, `node_modules/`, `dist/`, `.venv/`. Excluded directories that
the agent still wants to read get symlinked from the source project
after the workspace is created, so reads work without paying the copy
cost.

Binary files never enter the internal mirror. A git diff is meaningless
on binaries, and they would balloon the mirror for no benefit.

### Cleanup

Merged workspaces hang around for a configurable delay (default three
days) before being removed. This gives you a window to inspect what a
Task produced before it disappears. The cold store is unaffected by
workspace cleanup. Whatever got memorialized stays recallable.

## Completion pipeline

When a Task declares done, it enters a five-stage completion pipeline.

```
① Freeze
   worktree frozen (read-only)
   final summary generated
   heartbeat: ✅ Delivered
   Orchestrator + CLI notified

② Review
   risk assessment runs
   low risk -> rule checks only (compile, tests)
   medium/high risk -> Audit agent dispatched
   Orchestrator decides: auto-merge, forward to user

③ Merge
   conflict detection (predicted at dispatch time)
   no conflict -> merge directly
   conflict -> Orchestrator attempts auto-resolve
   auto-resolve fails -> ask the user

④ Memorialize
   final summary -> Orchestrator active memory
   full context + diff -> cold store (indexed)
   Orchestrator runs compaction

⑤ Cleanup
   worktree scheduled for delayed removal (default 3 days)
   cold store entries retained (memory is never lost)
```

### Risk grading

Risk drives whether Audit runs at all. Low-risk diffs (under 20 lines,
documentation-only) skip the Audit agent and run rule checks instead:
does it compile, do the tests pass. Medium and high-risk work gets a
full Audit pass. The threshold is configurable through
`audit.risk_threshold`.

### Dependency chain

Tasks can declare dependencies on other Tasks. When Task A finishes and
merges, any Task B that depended on A activates automatically. B gets a
fresh workspace based on the merged state of the main branch, so it
sees A's changes without a manual handoff.

### Continuation loop

A continuation chain is a sequence of Tasks where each completed Task
may trigger a new Task to continue unfinished work. The chain only
terminates when Audit returns `Confirmed`; `FalsePositive` and
`NeedsFix` trigger a new iteration carrying the audit findings as
feedback.

```
Task A (iteration 1)
  → pipeline: Freeze → Review → Merge → Memorialize → Cleanup
  → Audit verdict: NeedsFix
  → ContinuationCoordinator: budget ok, dispatch iteration 2

Task B (iteration 2, parent=A)
  → pipeline: Freeze → Review → Merge → Memorialize → Cleanup
  → Audit verdict: Confirmed
  → chain terminates: ConfirmedComplete
```

Each iteration is a **fresh Task** with its own workspace, provider,
and lifecycle. The state machine's `Delivered → Archived` direction is
preserved — continuation never revives a completed Task.

**Four-dimensional budget** bounds every chain:

| Dimension | Default | Effect |
|-----------|---------|--------|
| Max iterations | 10 | Hard cap on total Tasks in the chain |
| Max cost (USD) | 5.0 | Financial circuit breaker |
| Max duration | 30 min | Wall-clock timeout |
| Max no-progress rounds | 2 | Doom-loop detection |

Exhausting any dimension terminates the chain immediately with a
classified `ChainTerminationReason` that drives the user notification.

**Two-layer completion** (Sisyphus contract): a Task's self-reported
done claim is never final. An independent Audit agent must confirm the
work. `NeedsFix` with low confidence escalates to `NeedsHumanReview`,
stopping the chain and asking the user.

Users control chains with `/continue` (start or query), and
`/stop-continuation` (terminate). Configuration lives in the
`[continuation]` section of `.agent/config.toml`.

## Extension system

opca ships three separate extension points. They are deliberately
distinct rather than folded into one plugin API.

| Kind         | File                       | What it does                                   |
|--------------|----------------------------|------------------------------------------------|
| Context      | `AGENTS.md`, `skills/*.md` | Markdown injected into the system prompt.      |
| Capability   | `mcp.json`                 | Spawns MCP servers as child processes.         |
| Hook         | `hooks.toml`               | Lifecycle interception, some can block.        |

### Context

Context extensions are pure Markdown. A top-level `AGENTS.md` teaches
the agent how to think and act in this project. Skill files under
`skills/*.md` carry optional YAML frontmatter with metadata for
relevance matching, and a body that gets injected into a Task's system
prompt. Skills are scored against the Task description by keyword
overlap. A skill that shares no keywords with a Task does not load,
keeping the prompt small.

`AGENTS.md` supports `@import` lines that inline other files relative
to the importing file. Imports are depth-first, and each file is inlined
at most once, which breaks cycles.

### Capability

Capability extensions spawn one or more MCP (Model Context Protocol)
servers as child processes. Each server speaks JSON-RPC 2.0 over
stdin/stdout. The client drives three methods: `initialize` on spawn,
`tools/list` to enumerate tools, and `tools/call` to invoke one.

Tools from MCP servers are namespaced as `mcp__<server>__<tool>` to
avoid collisions with built-in tools and with tools from other servers.
Each server is a separate child process, so a crash isolates to that
server. The main agent process is unaffected.

### Hook

Hooks intercept the lifecycle. Four event levels are covered.

| Level         | Events                                                                                       | Can block      |
|---------------|----------------------------------------------------------------------------------------------|----------------|
| Session       | `on_session_start`, `on_session_end`                                                         | no             |
| Orchestrator  | `on_user_message`, `on_pre_dispatch`, `on_post_dispatch`, `on_task_highlight`, `on_recall`, `on_merge_pre`, `on_merge_post` | `merge_pre` only |
| Task          | `on_task_create`, `on_task_freeze`, `on_task_reject`, `on_task_archive`, `on_pre_tool_use`, `on_post_tool_use` | `pre_tool_use` only |
| Audit         | `on_audit_start`, `on_audit_report`, `on_audit_override`                                     | no             |

Only `on_pre_tool_use` and `on_merge_pre` honour a `Deny` result. On
every other event, a deny is logged but the operation proceeds. Hooks
that can block are what let rules graduate from suggestions to
enforcement.

Five handler types are recognized. `command` spawns a subprocess,
writes the payload to its stdin as JSON, and parses stdout to decide
the result. `http` POSTs the payload as JSON and parses the response
with the same shape rules. `mcp_tool`, `prompt`, and `agent` are
placeholders that log and return `Continue` until their downstream
dependencies are wired in.

The payload includes a `data` field with event-specific context. An
optional `matcher` substring filter on the hook config keeps hot paths
from spawning a subprocess on every event.

### Plugin packaging

A plugin is just a folder that bundles any combination of the three
extensions behind a single `plugin.toml` manifest. Plugins introduce no
new mechanisms. They exist for distribution. If you only need one
extension kind, you can skip plugins and drop the relevant file into
the project directly.

Plugins declare keywords so their tools only activate for relevant
Tasks. A plugin with no keywords is always on. Otherwise, the plugin
activates if any of its keywords appears as a substring of the
lowercased Task description. The Orchestrator makes the final call on
which plugins' tools to enable per Task.

The full plugin authoring guide, including a Docker helper walkthrough,
lives in [plugins.md](plugins.md).

## Provider

The `Provider` trait is the abstraction over every LLM backend. It is
also the pivot point that makes the rest of the system testable.

```rust
trait Provider {
    async fn stream(&self, messages: &[Message], tools: &[ToolDef])
        -> Stream<Event>;
}
```

Production implementations call real APIs. `AnthropicProvider` drives
the Messages API with SSE streaming. `OpenAIProvider` targets Chat
Completions. `GeminiProvider` targets Google Gemini. All three stream
events through the same `ProviderEvent` shape, so the agent loop does
not care which backend it is talking to.

`ScriptedProvider` is the test double. You program a response sequence
(then a tool call, then text, then done), and the orchestration tests
assert on how the system responds to a given LLM behavior. This is what
keeps LLM uncertainty out of unit tests. The tests answer "given the
LLM said X, what does the system do?", not "what does the LLM say?".

### Zero-copy context

Building the LLM context uses `Cow<'_, [Message]>` references instead of
cloning the full message vector. On large sessions this avoids a
noticeable allocation and copy on every turn. The context builder is
borrowed, not owned.

## Design decisions

The architecture above reflects twelve deliberate decisions, distilled
here as a summary. Each one was chosen over a real alternative.

| Decision                                       | Choice                                            | Rejected alternative                | Why                                                                              |
|------------------------------------------------|---------------------------------------------------|-------------------------------------|----------------------------------------------------------------------------------|
| Async runtime                                  | tokio                                             | asupersync, async-std               | Most mature, best documented, long-term maintenance.                            |
| Process model                                  | Single process, tokio tasks, channels             | Multi-process, IPC                  | Shared state is cheap in-process. Crash isolation via JoinHandle panic capture. |
| Non-blocking CLI                               | Silent background, completion notice, manual query| Dual-pane TUI                       | Simplest to ship. Covers most of the experience. Upgradable later.              |
| Lifecycle naming                               | Personified states (Sleeping, OnIt, ...)          | Clinical states (Created, Running)  | Scannable task list. Maps onto real work.                                        |
| Context layering                               | Three layers, Orchestrator sees 1 and 2           | Single flat context                 | Orchestrator manages many Tasks without context bloat.                           |
| Memory                                         | Fractal `Memory<T>` at three levels               | Separate stores per level           | Compaction and recall written once, reused everywhere.                           |
| Workspace isolation                            | Git worktree plus internal mirror plus copy       | Single strategy                     | Git projects get native worktrees; non-git still gets isolation.                 |
| Audit                                          | Read-only Task specialization                     | Independent top-level concept       | Reuses Task abstraction, memory, focus, highlights. Config difference only.      |
| Completion pipeline                            | 5-stage (Freeze, Review, Merge, Memorialize, Clean)| Ad-hoc handling                    | Predictable, stages are independently testable.                                  |
| Extension system                               | Three distinct points, plugins as packaging       | Unified plugin API                  | No product in the space uses a unified API. Plugins are bundles, not mechanisms. |
| Testing                                        | TDD, three layers, Provider trait as pivot        | Integration tests only              | LLM uncertainty isolated behind a trait. Unit tests stay deterministic.          |
| Borrowed designs from pi_agent_rust            | ToolEffects, steering and follow-up, zero-copy    | Fork the project                    | Fork binds to a single-maintainer runtime. Borrow ideas, not code.               |

## Open questions

A few things are unresolved and may shift. Surface them if your work
touches them.

1. **Semantic embedding model** for the cold store. Local (candle plus
   all-MiniLM) versus API (OpenAI embeddings). Affects offline
   availability.
2. **Workspace cleanup delay default.** Currently three days. Needs
   real user feedback to tune.
3. **CLI line editor.** `reedline` today. Open whether to switch to
   `rustyline` or hand-roll for finer control.
4. **Session persistence shape.** JSONL primary plus SQLite index today.
   Open whether to fold the index into the JSONL reader instead.
5. **MCP SDK.** `rmcp` is the Rust MCP SDK. The client is hand-rolled
   in `extensions/mcp.rs`; open whether to migrate.
