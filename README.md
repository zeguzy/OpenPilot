[English](README.md) | [中文](README.zh-CN.md)

# opca

**Background-first code agent in Rust.**

Dispatch long-running work to background workers, keep chatting in the
foreground, and never wait on a single agent turn. An Orchestrator routes
your messages, several Tasks run in parallel inside isolated workspaces,
and a separate Audit agent verifies finished work before it merges.

Most coding agents run a serial REPL. You submit a prompt, then sit and
wait while the agent chews through tool calls for several minutes. opca
flips that model. Quick questions get an instant reply from the
Orchestrator. Anything that looks slow gets dispatched to a background
Task with its own workspace, its own context window, and a personified
lifecycle you can query at any time. You keep talking. When a Task
finishes, you get a notification, a diff, and a verdict from an
independent Audit agent.

The three things that make this work:

- A **Focus Contract** between the Orchestrator and each Task, so the
  main brain only sees what matters instead of drowning in full logs.
- **Three-layer context** per Task (heartbeat, highlights, full history),
  with the Orchestrator pulling detail on demand via `deep_dive`.
- **Workspace isolation** through git worktrees, internal git mirrors,
  or plain copies, so parallel Tasks never step on each other.

## Quick start

```sh
# Build from source (MSRV 1.85)
git clone https://github.com/vhyc/openpilot-agent
cd openpilot-agent
cargo build --release

# Set an API key
export ANTHROPIC_API_KEY="sk-ant-..."

# Run it
./target/release/opca-cli --model claude-sonnet-4-20250514
```

OpenAI and Gemini work too. Set `OPENAI_API_KEY` or `GEMINI_API_KEY` and
pass the matching model id. See [Configuration](docs/configuration.md)
for the full key and model list, plus `.agent/config.toml` options.

For a guided walkthrough, read
[Getting started](docs/getting-started.md).

## Key features

- 🔨 **Background-first.** Every long task runs in the background. The
  foreground REPL never blocks. You can dispatch three tasks, then ask
  a quick question and get an answer before any of them finish.
- 🧠 **Three-role architecture.** An Orchestrator routes and decides,
  Tasks do the work, and an Audit agent verifies the result. Each role
  has its own context window and provider.
- 💤 **Personified lifecycle.** Tasks move through named states:
  Sleeping, Waking, Pondering, OnIt, Waiting, Reviewing, Delivered,
  Stuck, Axed, Archived. Every transition pushes a heartbeat
  automatically, so you always know what a Task is doing.
- 📊 **Three-layer context with Focus Contracts.** Each Task keeps a
  heartbeat (~50 tokens), highlights (~100 to 300 tokens), and a full
  history. The Orchestrator signs a Focus Contract saying which
  dimensions to report, and pulls the full stream only when it needs
  detail.
- 🗂️ **Fractal memory.** The same `Memory<T>` shape is reused at every
  level: active context, a compacted archive in SQLite, and a
  cross-session cold store. `recall` searches by keyword, time, task,
  or tag.
- 🔒 **Workspace isolation.** Git projects get native worktrees. Non-git
  projects get an internal git mirror with Copy-on-Write acceleration
  on APFS, btrfs, and xfs. A full copy is the last-resort fallback.
- 🔍 **Independent Audit agent.** A read-only specialization of Task.
  It reads the diff, runs the tests, and returns a pass, warn, or fail
  verdict with a confidence score. The Orchestrator can override it.
- 🔌 **Three extension points.** Context (Markdown injected into the
  prompt), Capability (MCP servers as child processes), and Hooks
  (lifecycle interception that can block operations). Plugins bundle
  these three for distribution.
- ⚡ **TDD-driven.** 590-plus tests across three layers. Zero clippy
  warnings under `pedantic` and `nursery`. The `Provider` trait is the
  pivot point that keeps LLM uncertainty out of unit tests.
- 🦀 **Rust all the way down.** `unsafe_code` is forbidden at the
  workspace level. Single process, tokio tasks, channel communication.
  No GC pauses, no runtime panics in library code.

## Architecture

```
CLI (foreground, never blocks)
   ↕ mpsc channel
Orchestrator (the "main brain")
   ├─ Active Memory (context window)
   ├─ Archive + Cold Store (SQLite indices)
   ├─ Task Registry (state of every active Task)
   └─ Tools: recall, deep_dive, dispatch, update_focus
        ↕ per-task channels (steering + heartbeats)
Tasks (tokio::spawn, N in parallel)
   ├─ Agent Loop
   ├─ Memory<Message>
   ├─ Workspace (git worktree / mirror / copy)
   └─ Tool Registry
        ↕
Audit (tokio::spawn, on demand)
   └─ read-only diff + test runs + verdict
```

Three roles collaborate inside a single process. The Orchestrator is
the only thing the user talks to. It routes messages, dispatches Tasks,
aggregates their heartbeats, and decides whether finished work merges,
needs audit, or asks the user.

Every Task lives in its own workspace with its own provider, memory, and
tool registry. Tasks report up through three layers: an automatic
heartbeat, on-demand highlights, and a full message history the
Orchestrator can pull via `deep_dive`.

The Audit agent is a read-only Task. It gets spawned on demand, reads
the diff, runs the tests, returns a verdict, and dies. It defaults to a
cheap model for cost control.

For the full engineering breakdown, including the state machine, the
completion pipeline, and the design decisions behind each subsystem,
read [Architecture](docs/architecture.md).

## Project structure

```
opca-core/      the library (13 modules)
  src/
    lifecycle/    TaskStatus state machine, heartbeat, spawn_task
    memory/       generic Memory<T>, Store, RecallQuery, compaction
    provider/     Provider trait, Message, Anthropic/OpenAI/Gemini
    workspace/    Git/Mirror/Copy isolation, CoW, agentignore
    focus/        FocusContract, Highlight, report_highlight
    task/         agent run loop, steering/follow-up queues
    orchestrator/ routing, dispatch, recall, heartbeat aggregation
    audit/        AuditAgent, verdict, override
    completion/   5-stage pipeline coordinator
    extensions/   Context + Capability + Hook, MCP client, plugins
    tools/        built-in tools (read, write, bash, ...)
    session/      JSONL writer, SQLite index, cold store
    di/           dependency-injection traits

opca-cli/       the binary
  src/
    main.rs        CLI parsing, banner, runtime entry
    repl.rs        line-editing REPL, main loop
    commands.rs    slash-command parser (/tasks, /status, /accept, ...)
    real.rs        wires opca-core to the CLI
    mock.rs        MockOrchestrator for demos and tests

opca-test-utils/   shared test fixtures
```

One concept per module. Files that cross 400 lines get split. The
canonical path for a type is `opca_core::<area>::<Type>`.

## Configuration

opca reads three config layers, in order of precedence: CLI flags,
environment variables, then `.agent/config.toml` in the project root.
The config file controls the default model, the audit model, workspace
isolation strategy, cleanup delay, memory caps, and audit risk
threshold.

Full reference: [docs/configuration.md](docs/configuration.md).

## Plugins

Plugins bundle the three extension points (Context, Capability, Hook)
behind a single `plugin.toml` manifest. You can teach the agent project
conventions through Markdown, expose new tools via MCP servers, and
enforce rules with hooks that block unsafe operations.

Authoring guide: [docs/plugins.md](docs/plugins.md).

## Development

```sh
cargo build                              # debug build
cargo test --workspace                   # run all tests
cargo clippy --workspace --all-targets   # lint, must be clean
cargo fmt --all -- --check               # verify formatting
```

The MSRV is **1.85**, edition **2024**. `unsafe_code` is forbidden at
the workspace level. Clippy runs at `pedantic` plus `nursery` with a
short allowlist tuned in `Cargo.toml`. Tests span three layers: pure
unit tests for infrastructure, `ScriptedProvider` plus `MockWorkspace`
for orchestration logic, and gated smoke tests for real provider calls.

See [AGENTS.md](AGENTS.md) for the conventions the project holds itself
to, including the TDD workflow, commit rules, and module layout.

## License

MIT
