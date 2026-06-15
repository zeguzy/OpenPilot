# AGENTS.md

This file orients the agent working on the **opca** codebase itself. It
is dogfooding: opca is a coding agent, and this is the project context
its own Tasks see when they work on the project.

## Project description

`opca` is a **background-first code agent** written in Rust. Users
dispatch long-running tasks to background workers, keep chatting in
the foreground, and never block on a single agent turn. An
Orchestrator routes messages, multiple Tasks run in parallel inside
isolated workspaces, and a separate Audit agent verifies finished work
before it merges.

The codebase is a Cargo workspace with three crates. See the
[module map](#module-structure) below for what lives where.

References:

- `openspec/changes/create-background-code-agent/design.md` — full
  architecture, decision record, risks.
- `openspec/changes/create-background-code-agent/proposal.md` —
  project description and capability list.
- `docs/configuration.md` — runtime configuration reference.
- `docs/plugins.md` — plugin authoring guide.

## Build commands

```sh
cargo build                         # debug build of all crates
cargo build --release               # optimised build
cargo test --workspace              # run every test in every crate
cargo test --workspace --no-run     # compile tests without running
cargo clippy --workspace --all-targets  # lint, must be clean
cargo fmt --all                     # format, must be a no-op
cargo fmt --all -- --check          # verify formatting in CI
cargo run --bin opca-cli -- --help  # run the CLI to see help
```

The MSRV is **1.85**, edition **2024**. `unsafe_code` is forbidden at
the workspace level. Clippy runs at `pedantic` + `nursery`, with a
small allowlist tuned in `Cargo.toml`.

## Architecture overview

Three roles collaborate inside a single process:

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

### Roles

- **Orchestrator.** One per session. Routes user messages (quick reply
  in the foreground vs long work dispatched to the background),
  aggregates Task heartbeats, manages Focus Contracts, recalls from
  the Cold Store, and decides whether each completed Task merges,
  needs audit, or asks the user.
- **Task.** A background worker. Owns its own Provider, Workspace,
  `Memory<Message>`, Focus Contract, and tool registry. Reports up via
  heartbeats (Layer 1, ~50 tokens), highlights (Layer 2, ~100-300
  tokens), and full message history (Layer 3, accessed on demand via
  `deep_dive`).
- **Audit.** A read-only specialisation of Task (`role=Auditor,
  readonly=true`). Spawn → Inspect → Judge → Report → Die. Defaults
  to a cheap model for cost control. The Orchestrator can override
  its verdict.

### Lifecycle

Every Task moves through the same state machine. State transitions
push a heartbeat automatically.

```
💤 Sleeping → 🌅 Waking → 🤔 Pondering → 🔨 OnIt
   ↓ on completion        ↓ on input needed
✅ Delivered              🫥 Waiting
   ↓ risk assessment
🔍 Reviewing → 📦 Archived
😵 Stuck → ✂️ Axed → 📦 Archived
```

### Workspace isolation

Each Task works inside its own workspace. The `WorkspaceManager`
auto-detects the right strategy:

1. Git project → `git worktree add`.
2. Non-git project → internal git mirror under `.agent/mirror/`.
3. Fallback → full directory copy.

Copy-on-Write (`clonefile` on APFS, `reflink` on btrfs/xfs) keeps
mirror creation cheap.

### Memory

Three regions, same `Memory<T>` shape at every level:

- **Active** — current context window.
- **Archive** — compacted history in SQLite with multi-dimensional
  indices (keyword, time, task_id, tag).
- **Cold Store** — cross-session persistent memory.

`compact()` moves items from Active to Archive; `recall()` searches
the archive; `remember()` writes to it.

## Coding conventions

These are non-negotiable. Automated checks enforce most of them.

### Rust

- **Forbid `unsafe`.** Set at the workspace level via
  `[workspace.lints.rust] unsafe_code = "forbid"`.
- **Clippy pedantic + nursery must be clean.** The allowlist in
  `Cargo.toml` is deliberately short. Don't add to it without
  justification in the commit message.
- **`cargo fmt --all` is a no-op.** Run it before every commit.
- **No panics in library code.** Return `Result`. The only `expect`
  calls live in tests.
- **Error types use `thiserror`.** Library errors implement
  `std::error::Error`. `anyhow` is fine at the binary edges.
- **Generics over dynamic dispatch where practical.** We do use
  `Box<dyn Trait>` for plugin/extension boundaries, but core data
  structures stay monomorphic.

### Hard Blocks (Task prompt enforcement)

The Task prompt's Phase 2 section includes a forbidden-actions list
("Hard Blocks") that the model is instructed to never violate. These
mirror the Rust conventions above and extend them to language-agnostic
anti-patterns. See `prompt_system/task/phase_2_execution.rs` for the
canonical list and `docs/prompt-system.md` for the full documentation.

### Prompt system

All LLM-facing prompt templates live under
`prompt_system/` (`crates/opca-core/src/prompt_system/`). Each area
exposes a `PROMPT_VERSION` constant logged at initialization. The
phase protocol (0-3), Evidence Gate, 3-strike rule, and Audit judgment
criteria are all defined here. See `docs/prompt-system.md` for the
module map, versioning policy, and instructions for adding new sections.

### Testing

- **TDD is the default.** Red → Green → Refactor.
- **Three layers.** Pure unit tests for infrastructure (Memory,
  Workspace, Lifecycle). ScriptedProvider + MockWorkspace for
  orchestration logic. Smoke tests for real provider integrations.
- **`rstest`** for parametric tests.
- **`insta`** for snapshot tests (heartbeats, audit reports, prompt
  templates). Review snapshots with `cargo insta review`.
- **`proptest`** for invariants (state machine transitions, Memory
  compaction preserves data).
- **Every public function has at least one test.** No exceptions.
- **No network in unit tests.** Real API calls live behind smoke
  tests gated on environment variables.

### Project structure

- **One concept per module.** If a file crosses 400 lines, split it.
- **Module-level docs.** Every `mod.rs` opens with a `//!` block that
  explains what the module does and points at the design.md section
  that justifies it.
- **Public API docs.** Every public item has a `///` doc comment. The
  comment explains *why*, not *what*; the code already says what.
- **Re-export from the crate root.** The canonical path for a type is
  `opca_core::<area>::<Type>`. Internal modules can rename with
  `r#trait` etc. to dodge reserved words.

### Commits and reviews

- **Atomic commits.** One logical change per commit. Tests pass at
  every commit.
- **No clippy suppressions.** Fix the warning, don't hide it. The
  only exception is `#[allow(dead_code)]` on fields used by tests,
  and only with a comment explaining why.
- **No new binary dependencies without workspace-level buy-in.** Add
  to `[workspace.dependencies]` first, then reference from the crate.

## Module structure

```
opca-core/                         the library
  src/
    lib.rs                         crate root, re-exports
    di.rs                          dependency-injection traits
    di/                            std impls of DI traits
    lifecycle/
      status.rs                    TaskStatus enum + transition table
      heartbeat.rs                 LifecycleTracker, auto-push on transition
      mod.rs                       spawn_task with panic capture
    memory/
      memory.rs                    generic Memory<T>
      store.rs                     Store (in-memory or file SQLite)
      index.rs                     RecallQuery (keyword/time/task/tag)
      compact.rs                   Threshold + Orchestrator compaction
    provider/
      provider.rs                  Provider trait, ProviderEvent
      message.rs                   Message, MessageRole
      tool.rs                      ToolDef, ToolEffects
      context.rs                   zero-copy ContextBuilder (Cow)
      anthropic.rs                 Anthropic Messages provider (SSE)
      openai.rs                    OpenAI Chat Completions provider
      gemini.rs                    Google Gemini provider
    workspace/
      trait.rs                     Workspace trait, ChangeSet, MergeResult
      git.rs                       GitWorkspace (git worktree)
      mirror.rs                    MirrorWorkspace (internal git mirror)
      copy.rs                      CopyWorkspace (full copy fallback)
      cow.rs                       CoW detection + copy_dir_cow
      agentignore.rs               .agentignore parser
      manager.rs                   WorkspaceManager (auto-detect)
      cleanup.rs                   delayed cleanup schedule
    focus/
      ...                          FocusContract, Highlight, report_highlight
    task/
      ...                          Task agent loop, steering/follow-up queues
    orchestrator/
      ...                          Orchestrator, route, dispatch, recall
    audit/
      ...                          AuditAgent, verdict, override
    completion/
      ...                          5-stage pipeline coordinator
    extensions/
      context.rs                   AGENTS.md + skills loaders
      hooks.rs                     Hook system (5 handler types)
      mcp.rs                       MCP client (JSON-RPC over stdio)
      plugin.rs                    plugin.toml manifest + installer
    tools/
      ...                          built-in tools (read/write/bash/...)
    session/
      format.rs                    SessionEntry, EntryKind
      writer.rs                    append-only JSONL writer
      reader.rs                    session log reloader
      index.rs                     SQLite metadata index
      cold_store.rs                cross-session recall archive

opca-cli/                          the binary
  src/
    main.rs                        CLI parsing, banner, runtime entry
    lib.rs                         OrchestratorApi trait, Reply, TaskInfo
    commands.rs                    slash-command parser, HELP_TEXT
    repl.rs                        Repl, Output trait, main loop
    mock.rs                        MockOrchestrator (no LLM, for tests/demos)
    real.rs                        RealOrchestrator (wires opca-core to CLI)
    tests/cli_integration.rs       end-to-end CLI tests

opca-test-utils/                   shared test fixtures
```

## Open design questions

These are unresolved. Don't pick a side unilaterally — surface them in
a Task highlight if your work touches them.

1. **Semantic embedding model** for the Cold Store. Local
   (`candle` + `all-MiniLM`) vs API (OpenAI embeddings). Affects offline
   availability.
2. **Workspace cleanup delay default.** Currently 3 days. Needs real
   user feedback to tune.
3. **CLI line editor.** `reedline` today. Open whether to switch to
   `rustyline` or hand-roll for finer control.
4. **Session persistence shape.** JSONL primary + SQLite index today.
   Open whether to fold the index into the JSONL reader instead.
5. **MCP SDK.** `rmcp` is the Rust MCP SDK. We hand-rolled the client
   in `extensions/mcp.rs`; open whether to migrate.
