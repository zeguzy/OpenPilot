[English](plugins.md) | [中文](plugins.zh-CN.md)

# 插件编写指南

`opca` 自带三个独立的扩展点。**插件**只是一个文件夹，把这些扩展的任意组合打包到一个 manifest 后面，让安装感觉像原子操作。这份指南覆盖 manifest、每个组件，以及一次完整的 walkthrough。

如果你只需要三种扩展中的一种，可以完全跳过插件，直接把相关文件（`AGENTS.md`、`hooks.toml` 等）丢进项目。插件是为分发而生的，不是为使用。

## 目录

- [三个扩展点](#三个扩展点)
- [插件布局](#插件布局)
- [`plugin.toml`](#plugintoml)
- [Skill 文件（`skills/*.md`）](#skill-文件skillsmd)
- [`hooks.toml`](#hookstoml)
- [`mcp.json`](#mcpjson)
- [实战：一个 Docker helper 插件](#实战一个-docker-helper-插件)
- [激活：按任务选工具](#激活按任务选工具)
- [测试插件](#测试插件)

## 三个扩展点

| 类型           | 文件                         | 作用                                                       |
|----------------|------------------------------|------------------------------------------------------------|
| **Context**    | `AGENTS.md`, `skills/*.md`   | 注入 system prompt 的纯 Markdown。教会 agent 怎么想、怎么做。 |
| **Capability** | `mcp.json`                   | 以子进程方式 spawn 一个或多个 MCP (Model Context Protocol) server。给 agent 加它能调的新工具。 |
| **Hook**       | `hooks.toml`                 | 生命周期拦截。在事件上触发外部命令、HTTP 调用或 LLM prompt。有些 hook 能阻断操作。 |

插件不引入新机制，它只把这三种打包起来。

## 插件布局

```
my-plugin/
  plugin.toml          # manifest, required
  AGENTS.md            # Context, optional
  skills/              # Context, optional
    *.md
  mcp.json             # Capability, optional
  hooks.toml           # Hook, optional
```

每个组件都是可选的。一个插件可以只发 Context、只发 Capability、只发 Hook，或任意组合。manifest 声明加载哪些文件。

## `plugin.toml`

manifest，TOML 格式。必填 key：`name`、`version`。其余都可选。

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

### 校验规则

- `name` 不能为空。
- `version` 不能为空。
- 组件路径相对于插件目录。
- 缺失的组件文件被静默容忍。格式错误的文件会中止整个安装，所以一个装到一半的插件绝不会漏进 agent。

## Skill 文件（`skills/*.md`）

skill 是带可选 YAML frontmatter 的 Markdown 文件。frontmatter 装的是用于相关性匹配的元数据，正文是注入任务 system prompt 的指令内容。

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

### frontmatter 字段

| 字段          | 必填 | 描述                                                          |
|---------------|------|---------------------------------------------------------------|
| `name`        | 否   | 稳定标识符。默认取文件名 stem。                               |
| `description` | 否   | 一行摘要，也会被分词成关键词。                                |
| `keywords`    | 否   | 逗号或空格分隔。自动转小写。                                  |

`keywords` 缺失时，加载器从文件 stem 和 `description` 的词里推导关键词。你只想覆盖默认时才需要设 `keywords`。

### 相关性匹配

skill 按关键词重叠对任务描述打分。一个 skill 在某个任务上若关键词零命中，就不为该任务加载，以此保持 system prompt 小巧。

### `AGENTS.md` 里的 `@import` 语法

顶层 `AGENTS.md` 支持通过以 `@` 开头的行内联其他文件：

```markdown
# Project conventions

@docs/coding-style.md
@docs/testing.md
```

路径相对于包含 import 的文件。导入是深度优先的，每次加载中每个文件最多内联一次，从而打断环。

## `hooks.toml`

一个带顶层 `[[hooks]]` 数组的 TOML 文件。每条订阅一个生命周期事件并派发给一个 handler。

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

### 事件

hook 在四个生命周期层级上触发。只有 `on_pre_tool_use` 和 `on_merge_pre` 遵守 `Deny` 结果，其他每个事件上 deny 只记日志，操作照常进行。

| 层级          | 事件                                                      | 遵守 deny     |
|---------------|-----------------------------------------------------------|---------------|
| Session       | `on_session_start`, `on_session_end`                      | 否            |
| Orchestrator  | `on_user_message`, `on_pre_dispatch`, `on_post_dispatch`, `on_task_highlight`, `on_recall`, `on_merge_pre`, `on_merge_post` | 仅 `merge_pre` |
| Task          | `on_task_create`, `on_task_freeze`, `on_task_reject`, `on_task_archive`, `on_pre_tool_use`, `on_post_tool_use` | 仅 `pre_tool_use` |
| Audit         | `on_audit_start`, `on_audit_report`, `on_audit_override`  | 否            |

### handler 类型

识别五种 handler 类型。`command` 和 `http` 今天已完全实现；`mcp_tool`、`prompt`、`agent` 是占位符，目前只记日志并返回 `Continue`，等下游依赖接通。

#### `command`

spawn 一个子进程。hook payload 作为 JSON 写到子进程的 stdin，子进程的 stdout 作为 JSON 解析来决定结果。

```toml
[hooks.handler]
type = "command"
command = "scripts/check.sh"
args = []
```

识别的 stdout 形状（字符串值大小写不敏感）：

- `{"result": "allow"}`，操作可继续。
- `{"result": "deny", "reason": "..."}`，操作被阻断。
- `{"result": "modify", "data": {...}}`，替换 payload 的一部分。
- `{"result": "continue"}`，弃权。
- 其他任何形状，或非零退出，默认按 `Deny` 处理并附上 stderr。

空 stdout 视作 `Allow`。非零退出码视作 `Deny`，并附上 stderr。

#### `http`

把 payload 作为 JSON POST 到一个 URL。响应体按和 `command` 同样的形状规则解析。

```toml
[hooks.handler]
type = "http"
url = "https://internal-hooks.example.com/opca/merge-pre"
```

#### `mcp_tool`、`prompt`、`agent`（占位符）

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

这些预留给将来使用。它们通过 `tracing::debug!` 记日志并返回 `Continue`，所以派发能在真正实现落地前就端到端跑通。

### hook payload

写到 stdin（或 POST 出去）的 payload 是一个 JSON 对象。确切形状取决于事件，但每个 payload 都带一个 `data` 字段，装着事件特定的上下文（task id、文件路径、工具名、diff 等）。hook config 上可选的 `matcher` 字段是对 JSON 编码后 payload 应用子串过滤，能让热路径不必在每个事件上都 spawn 子进程。

## `mcp.json`

声明一个或多个作为子进程 spawn 的 MCP server。每个 server 通过 stdin/stdout 说 JSON-RPC 2.0（每行一个 JSON 对象，换行分隔）。

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

### 字段

| 字段      | 必填 | 描述                                                         |
|-----------|------|--------------------------------------------------------------|
| `name`    | 是   | server 标识符。工具前缀为 `mcp__<name>__<tool>`。            |
| `command` | 是   | 要 spawn 的可执行文件。                                      |
| `args`    | 否   | 传给可执行文件的 argv。                                      |
| `env`     | 否   | 子进程的额外环境变量。                                       |

### 协议

client（`opca`）驱动三个 JSON-RPC 方法：

1. `initialize`，spawn 时发送。上报 `clientInfo` 并读取 server 上报的能力。
2. `tools/list`，枚举 server 暴露的工具。
3. `tools/call`，按名调用一个工具，带 JSON 参数。

Resources 和 prompts 发现目前不用。

### 崩溃隔离

每个 server 是一个子进程。如果它崩了，下次请求它会返回错误，agent 把该工具标记为不可用。主 agent 进程不受影响。

### 工具名命名空间

MCP 工具名加前缀，避免和内置工具及其他 server 的工具冲突：

```
mcp__<server-name>__<tool-name>
```

对上面的例子，`db-query` server 上的 `query` 工具在 agent 的工具注册表里显示为 `mcp__db-query__query`。

## 实战：一个 Docker helper 插件

这个插件教 agent 对容器构建保持谨慎，通过 MCP 暴露一个 compose-lint 工具，并阻断破坏性的 prune 命令。

### 目录布局

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

### `scripts/guard-docker-prune.sh`（位于宿主项目里）

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

## 激活：按任务选工具

插件声明 `keywords`，让它的工具只对相关任务激活，以此保持工具列表精简、system prompt 干净。

匹配规则：

- **没有** `keywords` 的插件永远激活。每个任务都需要的核心工具包用这个。
- 否则，当且仅当它的任一 keyword 作为子串出现在小写化的任务描述里时，插件才激活。用子串匹配是为了让 `container` 能在 "containerize the app" 上命中。

编排器有最终决定权。它为每个任务挑选启用哪些插件的工具，然后只把那些工具注入任务的工具注册表。

### 实例

| 插件 keywords                     | 任务描述                          | 激活？  |
|-----------------------------------|-----------------------------------|---------|
| `[]`（永远开启）                  | refactor auth module              | 是      |
| `docker`, `container`             | containerize the api              | 是      |
| `docker`, `container`             | refactor the auth module          | 否      |
| `rust`                            | add tests for the python cli      | 否      |
| `rust`, `python`                  | add tests for the python cli      | 是      |

## 测试插件

最快的测试方式是把插件装到一个 mock 项目上，再对结果 `InstalledPlugin` 字段做断言。`opca-core` 测试套件里有例子，在 `crates/opca-core/tests/extensions_plugin.rs` 下。

快速清单：

1. `plugin.toml` 能用 `PluginManifest::parse_toml` 解析。
2. manifest 里的每个组件路径都指向真实文件。
3. skill 有合法的 frontmatter（或干脆没有）。
4. hook 目标是真实事件（`as_config_key` 列全了）。
5. MCP server 在 30 秒内响应 `initialize`。

端到端 smoke 测试：让 `opca` 指向一个装了该插件的 scratch 项目，派发一个描述里含插件 keyword 的任务。`/tasks` 斜杠命令应该能看到任务在各生命周期状态间推进。
