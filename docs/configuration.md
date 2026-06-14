[English](configuration.md) | [中文](configuration.zh-CN.md)

# Configuration

`opca` is configured through three layers, applied in order of precedence:

1. **CLI flags** (highest precedence) — see `opca --help`.
2. **Environment variables** — useful for secrets and CI.
3. **Project config** — `.agent/config.toml` inside the project root.

This document covers layers 2 and 3, plus the related `.agentignore`
file and the workspace isolation strategies.

## Table of contents

- [Project layout](#project-layout)
- [`.agent/config.toml`](#agentconfigtoml)
- [`.agentignore`](#agentignore)
- [Environment variables](#environment-variables)
- [Workspace isolation strategies](#workspace-isolation-strategies)
- [Sessions and the Cold Store](#sessions-and-the-cold-store)

## Project layout

When you run `opca` inside a project, it owns a single directory at the
project root:

```
your-project/
  .agent/
    config.toml              # optional, project config
    mirror/                  # internal git mirror for non-git projects
    sessions/<id>.jsonl      # one JSONL log per session
    session-index.sqlite     # metadata index across sessions
    cold-store.sqlite        # cross-session recall archive
  .agentignore               # optional, same syntax as .gitignore
  AGENTS.md                  # optional, project-wide context for the agent
```

Nothing outside `.agent/` is written by the agent itself. Workspaces for
background Tasks live under the system temp directory (or the
`workspace_parent` you configure), never inside your project tree.

## `.agent/config.toml`

Project-level configuration. All keys are optional; missing keys fall
back to the defaults shown below.

```toml
# .agent/config.toml — all keys optional

[model]
# Default model id passed to the provider. Overridden by --model or
# OPCA_MODEL. Examples: "claude-sonnet-4-20250514", "gpt-4o",
# "gemini-1.5-pro".
default = "claude-sonnet-4-20250514"

# Model used by the Audit agent. Cheaper models keep audit costs down;
# escalate to a stronger model for high-risk tasks.
audit_model = "claude-haiku"

[provider]
# Provider kind. One of: "anthropic", "openai", "gemini", "stub".
# "stub" returns a canned response and never calls a real API — handy
# for demos and smoke tests.
kind = "anthropic"

[isolation]
# How each background Task gets its own workspace. One of:
#   "auto"         detect (git project -> git worktree, else git mirror)
#   "git-mirror"   always use the internal git mirror under .agent/mirror/
#   "copy"         always full-copy the project directory (slowest)
#   "none"         no isolation, Tasks run serially against the project
strategy = "auto"

# Parent directory under which Task workspaces live. Defaults to the
# system temp directory when unset.
# workspace_parent = "/tmp/opca-workspaces"

[cleanup]
# How long a merged workspace hangs around before being removed.
# Default: 3 days. Set to 0 for immediate cleanup.
delay_days = 3

[memory]
# Soft cap on the active context window (in approximate tokens) before
# the Orchestrator compacts older items into the archive.
max_active_tokens = 8000

[hooks]
# Default per-hook timeout in milliseconds. Individual hooks can override.
default_timeout_ms = 10000

[audit]
# Risk threshold above which an Audit agent is dispatched.
#   "low"      rule checks only (compile + tests)
#   "medium"   Audit agent on diffs > 20 lines or non-doc files
#   "high"     Audit agent on every completed Task
risk_threshold = "medium"
```

### Keys the runtime honours today

Not every key above is wired into the binary yet. The columns below track
which ones take effect in the current release.

| Key                  | Status      | Notes                                                          |
|----------------------|-------------|----------------------------------------------------------------|
| `model.default`      | read        | Surfaced through `--model` when the flag is absent.            |
| `model.audit_model`  | reserved    | Honoured once Audit dispatch is wired to the provider.         |
| `provider.kind`      | reserved    | Provider is currently injected by the CLI binary.              |
| `isolation.strategy` | read        | Maps to `IsolationStrategy` (`Auto`/`Git`/`Mirror`/`Copy`).   |
| `isolation.workspace_parent` | read | Passed to `WorkspaceManager::with_workspace_parent`.    |
| `cleanup.delay_days` | read        | Default 3 days. `0` schedules immediate removal.               |
| `memory.max_active_tokens` | read  | Caps the Orchestrator's active region.                       |
| `hooks.default_timeout_ms` | reserved | Per-hook `timeout_ms` already works in `hooks.toml`.        |
| `audit.risk_threshold` | reserved  | Risk classification runs, but the threshold is hard-coded.     |

Keys marked `reserved` document the intended contract so config files
stay forward-compatible.

## `.agentignore`

Same syntax as `.gitignore`. Patterns listed here are excluded from
workspace mirror imports and from `CopyWorkspace` directory walks. Put
your heavy build artifacts here so workspace creation stays fast.

```gitignore
# .agentignore

# Build outputs
target/
dist/
build/
node_modules/

# Virtual environments
.venv/
venv/

# Editor and OS cruft
.vscode/
.idea/
.DS_Store

# Large media — diff is meaningless on binaries
*.mp4
*.zip
```

Two extra behaviours worth knowing:

- **Directory symlinks.** Excluded directories that the agent still
  wants read access to (think `node_modules/`, `target/`) are
  symlinked from the source project after the workspace is created,
  so reads work without paying the copy cost.
- **Binary files.** Binary files are never imported into the internal
  git mirror — `git diff` is meaningless on them and they balloon the
  mirror. Only text files participate in diffing and merging.

Patterns are matched case-sensitively against forward-slash paths
regardless of platform. Empty lines and lines starting with `#` are
ignored. Negation (`!`) works the same way as `.gitignore`.

## Environment variables

| Variable              | Purpose                                                       |
|-----------------------|---------------------------------------------------------------|
| `ANTHROPIC_API_KEY`   | API key for the Anthropic Messages provider.                  |
| `OPENAI_API_KEY`      | API key for the OpenAI Chat Completions provider.             |
| `GEMINI_API_KEY`      | API key for the Google Gemini provider.                       |
| `OPCA_MODEL`          | Default model id. Overridden by `--model`.                    |
| `OPCA_PROJECT`        | Default project path when `--project` is not passed.          |
| `OPCA_WORKSPACE_PARENT` | Parent dir for Task workspaces.                             |
| `RUST_LOG`            | Overrides the `-v` / `-vv` / `-vvv` filter when set.          |
| `NO_COLOR`            | Disable ANSI colour in log output.                            |

The API key variables are read by the provider constructors
(`AnthropicProvider::new`, `OpenAIProvider::new`, `GeminiProvider::new`)
when the CLI wires them up. Set them in your shell rc file or via a
`.env` loader — the binary does not read `.env` files itself.

```sh
# Example: export once per shell session
export ANTHROPIC_API_KEY="sk-ant-..."
opca --model claude-sonnet-4-20250514
```

## Workspace isolation strategies

Each background Task works inside its own workspace so concurrent Tasks
never step on each other. Pick the strategy that matches your project.

### `auto` (default)

- Project has a `.git` directory → uses **git worktree**.
- Otherwise → uses the **internal git mirror**.
- Falls back to **full copy** if mirror creation fails.

This is what you want 95% of the time.

### `git-mirror` (non-git projects)

Creates a fresh git repository under `.agent/mirror/`, imports the
project's text files (skipping `.agentignore` patterns and binary
files), then checks out a worktree per Task. Uses Copy-on-Write when
the filesystem supports it:

- macOS APFS — `clonefile` via `cp -R -c`.
- Linux btrfs / xfs — `reflink` via `cp -R --reflink=auto`.
- Other filesystems — full recursive copy.

Merge flow for non-git projects: extract the worktree diff, apply the
patch to the original project directory, then refresh the mirror
baseline.

### `copy` (fallback)

Full recursive directory copy. Slowest, highest disk usage, but works
everywhere and has no git dependency. Use this when git is unavailable
or the project shape confuses the mirror importer.

### `none`

No isolation. Tasks run serially against the live project directory.
Useful for read-only inspection work or when you trust a single Task
enough to skip the workspace ceremony. Background dispatch is
effectively serialised in this mode.

### Picking a strategy

```toml
# .agent/config.toml
[isolation]
strategy = "git-mirror"   # force mirror even for git projects
# strategy = "copy"       # safest, slowest
# strategy = "none"       # no isolation, serial Tasks
```

Or override per invocation:

```sh
opca --project .  # strategy comes from config or defaults to "auto"
```

## Sessions and the Cold Store

`opca` persists three things under `.agent/`:

| File                          | Holds                                            |
|-------------------------------|--------------------------------------------------|
| `sessions/<id>.jsonl`         | Append-only entry stream for one session.        |
| `session-index.sqlite`        | Per-session metadata, used for "resume" menus.   |
| `cold-store.sqlite`           | Cross-session recall archive (long-term memory). |

JSONL is human-inspectable and git-diffable. SQLite indexes the
metadata so listing or resuming sessions doesn't have to parse every
log file. The Cold Store survives across sessions: anything the
Orchestrator archives remains recallable from later sessions via the
`recall` tool.

Resume a previous session with:

```sh
opca --session 01HGE9R1K7QX...
```

Omit `--session` to start a new one. The session id is printed at
startup when verbose logging is on (`-v`).
