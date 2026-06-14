[English](getting-started.md) | [中文](getting-started.zh-CN.md)

# Getting started

This guide walks through installing opca, configuring it, and running
your first session. By the end you will have dispatched a background
task, queried its progress while chatting, and accepted the result.

If you just want the pitch first, read the
[README](../README.md). For the full engineering breakdown, read
[Architecture](architecture.md).

## Prerequisites

You need three things on the machine before you start.

- **Rust 1.85 or newer.** opca uses edition 2024. Check with
  `rustc --version`. Install or update via
  [rustup](https://rustup.rs/) if needed.
- **git.** Used for worktree isolation in git projects and the internal
  mirror in non-git projects. Verify with `git --version`.
- **An API key** for one of the supported providers. Pick one to start.

| Provider  | Environment variable  | Example model id              |
|-----------|-----------------------|-------------------------------|
| Anthropic | `ANTHROPIC_API_KEY`   | `claude-sonnet-4-20250514`    |
| OpenAI    | `OPENAI_API_KEY`      | `gpt-4o`                      |
| Gemini    | `GEMINI_API_KEY`      | `gemini-1.5-pro`              |

You only need one. The provider is injected by the CLI, so you pick it
when you launch.

## Installation

opca is not on crates.io yet. Build from source.

```sh
git clone https://github.com/vhyc/openpilot-agent
cd openpilot-agent
cargo build --release
```

The release binary lands at `target/release/opca-cli`. Copy it onto your
`PATH` if you want to call it as `opca`:

```sh
cp target/release/opca-cli ~/.local/bin/opca
```

Verify it runs:

```sh
opca --help
```

A debug build works fine for trying things out, but release mode is
noticeably snappier once you have real sessions going.

## Configuration

opca reads configuration in three layers, applied in this order of
precedence: CLI flags (highest), environment variables, then
`.agent/config.toml` in the project root. The full reference lives in
[configuration.md](configuration.md). This section covers the minimum
to get moving.

### Set your API key

Export it in your shell. Add the line to your shell rc file
(`~/.zshrc`, `~/.bashrc`) so it persists.

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
```

The binary does not read `.env` files. If you prefer a dotenv loader,
run it yourself before launching opca.

### Optional: project config

Create `.agent/config.toml` in the project you want opca to work on.
Every key is optional.

```toml
# .agent/config.toml

[model]
default = "claude-sonnet-4-20250514"
audit_model = "claude-haiku"

[isolation]
strategy = "auto"          # auto-detect: git -> worktree, else mirror
# workspace_parent = "/tmp/opca-workspaces"

[cleanup]
delay_days = 3

[memory]
max_active_tokens = 8000

[audit]
risk_threshold = "medium"  # low | medium | high
```

That is enough for a first run. The defaults are sensible, so you can
also skip this file entirely and pass everything through flags.

## Your first session

Start opca inside a project directory. Point it at the project and a
model.

```sh
opca --project . --model claude-sonnet-4-20250514
```

You should see the banner, then a prompt. The REPL is always ready for
input. Nothing in the foreground ever blocks.

### Ask a quick question

Type a question that does not need file changes.

```
> what does the memory module do?
```

The Orchestrator answers directly from its active context. No task gets
dispatched. This is the fast path: message in, reply out.

### Dispatch a background task

Now ask for something that takes real work.

```
> refactor the memory module to split store.rs into two files
```

The Orchestrator routes this to the background. It signs a Focus
Contract, creates an isolated workspace, and spawns a Task. You will see
a line like:

```
🔨 OnIt: task-0 is working on the refactor
```

That is the only output. The Task runs silently from here. You are free
to keep typing.

### Keep chatting while the task runs

The whole point of opca is that the foreground stays yours. While
task-0 churns on the refactor, ask something else.

```
> remind me, how does the lifecycle heartbeat work?
```

The Orchestrator replies straight away. The background Task is
unaffected.

### Query progress

Check on your task any time, either naturally or with a slash command.

```
> how is task-0 going?
```

The Orchestrator pulls the latest heartbeat from the Task Registry and
tells you the current state, what the Task is doing right now, and any
highlights it has reported.

The structured equivalent:

```
> /status task-0
```

List every active task at once:

```
> /tasks
```

### Handle a completed task

When a Task finishes, you get a completion notification.

```
🔔 task-0 finished: refactored store.rs into store.rs and index_impl.rs
   4 files changed, 312 insertions(+), 89 deletions(-)
   Audit verdict: warn (confidence 0.82)
```

Low-risk work may merge automatically. Medium and high-risk work waits
for your call. Review the diff in the workspace, then accept or reject.

Accept the result:

```
> /accept task-0
```

The Task merges into your project, the Orchestrator memorializes the
full context into the cold store, and the workspace schedules cleanup.

Reject it, with optional feedback that sends it back to `OnIt`:

```
> /reject task-0 "keep the index logic inside store.rs"
```

With feedback, the Task wakes up, reads the note, and tries again. Without
feedback, it gets axed and archived.

## Slash commands

| Command                  | What it does                                          |
|--------------------------|-------------------------------------------------------|
| `/tasks`                 | List every active task and its current state.         |
| `/status [task-id]`      | Show one task in detail, or an overview if no id.     |
| `/accept <task-id>`      | Accept and merge a delivered task.                    |
| `/reject <task-id> [msg]`| Reject a task. With a message, it returns to OnIt.    |
| `/help`                  | Show the help text.                                   |
| `/quit`                  | Exit the REPL.                                        |

You do not have to memorize these. Natural language works for
everything except accept and reject, which need an explicit task id.
Ask "what's running?" and the Orchestrator interprets it.

## Next steps

- **[Configuration](configuration.md)** tunes the model, isolation
  strategy, memory caps, cleanup delay, and audit thresholds. Read it
  when you want to move past the defaults.
- **[Plugin authoring guide](plugins.md)** shows how to bundle Context,
  Capability, and Hook extensions into a distributable plugin.
- **[Architecture](architecture.md)** explains the three-role model, the
  lifecycle state machine, the three-layer context, and the design
  decisions behind every subsystem.
