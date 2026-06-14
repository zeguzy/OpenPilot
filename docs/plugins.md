[English](plugins.md) | [中文](plugins.zh-CN.md)

# Plugin authoring guide

`opca` ships with three separate extension points. A **plugin** is just a
folder that bundles any combination of those extensions behind a single
manifest, so installing one feels atomic. This guide covers the manifest,
each component, and a full walkthrough.

If you only need one of the three extension kinds, you can skip plugins
entirely and drop the relevant file (an `AGENTS.md`, a `hooks.toml`,
etc.) into the project. Plugins are for distribution, not for use.

## Table of contents

- [The three extension points](#the-three-extension-points)
- [Plugin layout](#plugin-layout)
- [`plugin.toml`](#plugintoml)
- [Skill files (`skills/*.md`)](#skill-files-skillsmd)
- [`hooks.toml`](#hookstoml)
- [`mcp.json`](#mcpjson)
- [Walkthrough: a Docker helper plugin](#walkthrough-a-docker-helper-plugin)
- [Activation: per-Task tool selection](#activation-per-task-tool-selection)
- [Testing plugins](#testing-plugins)

## The three extension points

| Kind         | File              | What it does                                    |
|--------------|-------------------|-------------------------------------------------|
| **Context**  | `AGENTS.md`, `skills/*.md` | Pure Markdown injected into the system prompt. Teaches the agent how to think and act. |
| **Capability** | `mcp.json`      | Spawns one or more MCP (Model Context Protocol) servers as child processes. Adds new tools the agent can call. |
| **Hook**     | `hooks.toml`      | Lifecycle interception. External commands, HTTP calls, or LLM prompts fired on events. Some hooks can block operations. |

Plugins introduce no new mechanisms. They only bundle these three.

## Plugin layout

```
my-plugin/
  plugin.toml          # manifest, required
  AGENTS.md            # Context, optional
  skills/              # Context, optional
    *.md
  mcp.json             # Capability, optional
  hooks.toml           # Hook, optional
```

Every component is optional. A plugin can ship Context only, Capability
only, Hook only, or any combination. The manifest declares which files
to load.

## `plugin.toml`

The manifest. TOML format. Required keys: `name`, `version`. Everything
else is optional.

```toml
# plugin.toml

name = "docker-helper"
version = "0.1.0"
author = "Your Name <you@example.com>"

# Optional: keywords used to decide when this plugin's tools activate.
# If omitted, the plugin is always on. See "Activation" below.
keywords = ["docker", "container", "compose"]

# Optional: paths are relative to this plugin's directory.
context = "AGENTS.md"        # Markdown injected into the system prompt
skills = "skills"            # Directory of *.md skill files
mcp    = "mcp.json"          # MCP server config
hooks  = "hooks.toml"        # Hook definitions
```

### Validation rules

- `name` must be non-empty.
- `version` must be non-empty.
- Component paths are relative to the plugin directory.
- Missing component files are silently tolerated. Malformed files abort
  the whole install, so a half-installed plugin never leaks into the
  agent.

## Skill files (`skills/*.md`)

Skills are Markdown files with optional YAML frontmatter. The
frontmatter carries metadata for relevance matching; the body is the
instruction content injected into a Task's system prompt.

```markdown
---
name: rust-refactor
description: Refactor Rust code following project conventions
keywords: rust, refactor, module, trait
---

# Rust refactoring skill

When asked to refactor Rust code:

1. Run `cargo check` before and after every change.
2. Prefer splitting large modules into submodules rather than leaving
   long files.
3. Never use `unsafe` unless the existing code already requires it.
4. Run `cargo clippy --workspace -- -D warnings` before declaring done.

Treat `#[forbid(unsafe_code)]` as a project-wide rule.
```

### Frontmatter fields

| Field         | Required | Description                                                  |
|---------------|----------|--------------------------------------------------------------|
| `name`        | no       | Stable identifier. Defaults to the file stem.                |
| `description` | no       | One-line summary, also tokenised into keywords.              |
| `keywords`    | no       | Comma- or space-separated. Lowercased automatically.         |

If `keywords` is missing, the loader derives keywords from the file
stem and the words in `description`. You only need to set `keywords`
when you want to override that default.

### Relevance matching

Skills are scored against the Task description by counting keyword
overlap. A skill scores zero on a given Task if none of its keywords
appear in the Task description — it is not loaded for that Task. This
keeps the system prompt small.

### `@import` syntax in AGENTS.md

Top-level `AGENTS.md` supports inlining other files via a line that
starts with `@`:

```markdown
# Project conventions

@docs/coding-style.md
@docs/testing.md
```

Paths are relative to the file containing the import. Imports are
depth-first; each file is inlined at most once per load, which breaks
cycles.

## `hooks.toml`

A TOML file with a top-level `[[hooks]]` array. Each entry subscribes
to one lifecycle event and dispatches to a handler.

```toml
# hooks.toml

[[hooks]]
event = "pre_tool_use"          # see event table below
matcher = "rm -rf"              # optional substring filter on payload
timeout_ms = 5000               # optional, default 10000
can_block = true                # optional, default true

  [hooks.handler]
  type = "command"              # one of: command, http, mcp_tool, prompt, agent
  command = "bash"
  args = ["-c", "echo denied 1>&2 && exit 1"]

[[hooks]]
event = "merge_pre"
can_block = true

  [hooks.handler]
  type = "command"
  command = "cargo"
  args = ["test", "--workspace"]
```

### Events

Hooks fire across four lifecycle levels. Only `on_pre_tool_use` and
`on_merge_pre` honour a `Deny` result; on every other event a deny is
logged but the operation proceeds.

| Level          | Event                                                   | Honours deny |
|----------------|---------------------------------------------------------|--------------|
| Session        | `on_session_start`, `on_session_end`                    | no           |
| Orchestrator   | `on_user_message`, `on_pre_dispatch`, `on_post_dispatch`, `on_task_highlight`, `on_recall`, `on_merge_pre`, `on_merge_post` | `merge_pre` only |
| Task           | `on_task_create`, `on_task_freeze`, `on_task_reject`, `on_task_archive`, `on_pre_tool_use`, `on_post_tool_use` | `pre_tool_use` only |
| Audit          | `on_audit_start`, `on_audit_report`, `on_audit_override` | no           |

### Handler types

Five handler types are recognised. `command` and `http` are fully
implemented today; `mcp_tool`, `prompt`, and `agent` are placeholders
that log and return `Continue` until their downstream dependencies
are wired in.

#### `command`

Spawn a subprocess. The hook payload is written to the child's stdin
as JSON; the child's stdout is parsed as JSON to decide the result.

```toml
[hooks.handler]
type = "command"
command = "scripts/check.sh"
args = []
```

Recognised stdout shapes (case-insensitive on string values):

- `{"result": "allow"}` — operation may proceed.
- `{"result": "deny", "reason": "..."}` — operation blocked.
- `{"result": "modify", "data": {...}}` — replace part of the payload.
- `{"result": "continue"}` — abstain.
- Any other shape, or a non-zero exit, defaults to `Deny` with the
  stderr attached.

Empty stdout is treated as `Allow`. A non-zero exit code is treated as
`Deny` with the stderr attached.

#### `http`

POST the payload as JSON to a URL. The response body is parsed with
the same shape rules as `command`.

```toml
[hooks.handler]
type = "http"
url = "https://internal-hooks.example.com/opca/merge-pre"
```

#### `mcp_tool`, `prompt`, `agent` (placeholders)

```toml
[hooks.handler]
type = "mcp_tool"
server = "policy-server"
tool = "check_merge"

# or

[hooks.handler]
type = "prompt"
template = "Is this diff safe to merge? Answer allow or deny."

# or

[hooks.handler]
type = "agent"
instruction = "Verify the merge does not break the auth module."
```

These are reserved for future use. They log via `tracing::debug!` and
return `Continue`, so dispatch can run end-to-end before the real
implementations land.

### Hook payload

The payload written to stdin (or POSTed) is a JSON object. The exact
shape depends on the event, but every payload includes a `data` field
carrying the event-specific context (task id, file path, tool name,
diff, etc.). The optional `matcher` field on the hook config is a
substring filter applied to the JSON-encoded payload, which keeps hot
paths from spawning a subprocess on every event.

## `mcp.json`

Declares one or more MCP servers to spawn as child processes. Each
server speaks JSON-RPC 2.0 over stdin/stdout (one JSON object per
line, newline-delimited).

```json
{
  "servers": [
    {
      "name": "db-query",
      "command": "python",
      "args": ["-m", "opca_db_query_server"],
      "env": {
        "DATABASE_URL": "postgres://localhost/app"
      }
    },
    {
      "name": "shell-tools",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-shell"]
    }
  ]
}
```

### Fields

| Field     | Required | Description                                                |
|-----------|----------|------------------------------------------------------------|
| `name`    | yes      | Server identifier. Tools are prefixed `mcp__<name>__<tool>`. |
| `command` | yes      | Executable to spawn.                                       |
| `args`    | no       | Argv passed to the executable.                             |
| `env`     | no       | Extra environment variables for the child process.         |

### Protocol

The client (`opca`) drives three JSON-RPC methods:

1. `initialize` — sent on spawn. Reports `clientInfo` and reads the
   server's reported capabilities.
2. `tools/list` — enumerates the tools the server exposes.
3. `tools/call` — invokes a tool by name with JSON arguments.

Resources and prompts discovery are not used today.

### Crash isolation

Each server is a child process. If it crashes, the next request to it
returns an error and the agent surfaces that tool as unavailable. The
main agent process is unaffected.

### Tool name namespacing

MCP tool names are prefixed to avoid collisions with built-in tools
and with tools from other servers:

```
mcp__<server-name>__<tool-name>
```

For the example above, a `query` tool on the `db-query` server appears
as `mcp__db-query__query` in the agent's tool registry.

## Walkthrough: a Docker helper plugin

This plugin teaches the agent to be careful with container builds,
exposes a compose-lint tool via MCP, and blocks destructive prune
commands.

### Directory layout

```
docker-helper/
  plugin.toml
  AGENTS.md
  skills/
    docker-build.md
  mcp.json
  hooks.toml
```

### `plugin.toml`

```toml
name = "docker-helper"
version = "0.1.0"
author = "ops-team@example.com"
keywords = ["docker", "container", "compose"]

context = "AGENTS.md"
skills  = "skills"
mcp     = "mcp.json"
hooks   = "hooks.toml"
```

### `AGENTS.md`

```markdown
# Docker conventions

- Pin base image digests, not just tags.
- Multi-stage builds are mandatory for production images.
- Never run containers as root. Create and switch to a non-root user.
- The compose file is the source of truth for local dev. Do not start
  ad-hoc containers when a compose service exists.
```

### `skills/docker-build.md`

```markdown
---
name: docker-build
description: Build and tag Docker images the project's way
keywords: docker, build, image, tag
---

# Docker build skill

1. Read `docker/Dockerfile.*` to find the right file.
2. Tag images as `<repo>:<git-sha>`, never as `latest`.
3. Run `docker build --target prod` to get the production image.
4. Scan with `trivy image` before declaring done.
```

### `mcp.json`

```json
{
  "servers": [
    {
      "name": "compose-lint",
      "command": "npx",
      "args": ["-y", "@opca/mcp-compose-lint"]
    }
  ]
}
```

### `hooks.toml`

```toml
# Block `docker system prune` and `docker volume rm` on the live dev env.
[[hooks]]
event = "pre_tool_use"
matcher = "docker"
can_block = true
timeout_ms = 3000

  [hooks.handler]
  type = "command"
  command = "bash"
  args = ["-c", "scripts/guard-docker-prune.sh"]

# Run compose config validation before any merge that touches compose files.
[[hooks]]
event = "merge_pre"
matcher = "docker-compose"
can_block = true

  [hooks.handler]
  type = "command"
  command = "docker"
  args = ["compose", "config", "--quiet"]
```

### `scripts/guard-docker-prune.sh` (lives in the host project)

```sh
#!/usr/bin/env bash
read -r payload
case "$payload" in
  *"system prune"*|*"volume rm"*)
    echo '{"result":"deny","reason":"destructive docker command"}'
    ;;
  *)
    echo '{"result":"allow"}'
    ;;
esac
```

## Activation: per-Task tool selection

Plugins declare `keywords` so their tools only activate for relevant
Tasks. This keeps the tool list small and the system prompt lean.

The matching rule:

- A plugin with **no** `keywords` is always activated. Use this for
  core tool packs that every Task needs.
- Otherwise, the plugin activates if and only if any of its keywords
  appears as a substring of the lowercased Task description. Substring
  matching so `container` activates for `containerize the app`.

The Orchestrator makes the final call. It picks which plugins'
tools to enable per Task, then injects only those tools into the
Task's tool registry.

### Worked examples

| Plugin keywords                  | Task description                  | Activated? |
|----------------------------------|-----------------------------------|------------|
| `[]` (always on)                 | refactor auth module              | yes        |
| `docker`, `container`            | containerize the api              | yes        |
| `docker`, `container`            | refactor the auth module          | no         |
| `rust`                           | add tests for the python cli      | no         |
| `rust`, `python`                 | add tests for the python cli      | yes        |

## Testing plugins

The fastest way to test a plugin is to install it against a mock
project and assert on the resulting `InstalledPlugin` fields. The
`opca-core` test suite has examples under
`crates/opca-core/tests/extensions_plugin.rs`.

Quick checklist:

1. `plugin.toml` parses with `PluginManifest::parse_toml`.
2. Every component path in the manifest resolves to a real file.
3. Skills have valid frontmatter (or none at all).
4. Hooks target real events (`as_config_key` lists them all).
5. MCP servers respond to `initialize` within 30 seconds.

For end-to-end smoke tests, point `opca` at a scratch project with
the plugin installed and dispatch a Task whose description contains
one of the plugin's keywords. The `/tasks` slash command should show
the Task progressing through lifecycle states.
