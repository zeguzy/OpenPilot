[English](configuration.md) | [中文](configuration.zh-CN.md)

# 配置

`opca` 通过三层配置，按以下优先级生效：

1. **CLI flag**（最高优先级），见 `opca --help`。
2. **环境变量**，适合放 secret 和 CI。
3. **项目配置**，项目根目录下的 `.agent/config.toml`。

这份文档覆盖第 2、3 层，外加相关的 `.agentignore` 文件和工作区隔离策略。

## 目录

- [项目布局](#项目布局)
- [`.agent/config.toml`](#agentconfigtoml)
- [`.agentignore`](#agentignore)
- [环境变量](#环境变量)
- [工作区隔离策略](#工作区隔离策略)
- [会话与冷存储](#会话与冷存储)

## 项目布局

在项目里跑 `opca` 时，它会在项目根目录下拥有一个目录：

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

`.agent/` 之外不会被 agent 自己写入。后台任务的工作区位于系统临时目录下（或你配置的 `workspace_parent`），绝不落在你的项目树里。

## `.agent/config.toml`

项目级配置。所有 key 都是可选的，缺失的 key 回落到下面展示的默认值。

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

### 运行时目前真正生效的 key

并非上面每个 key 都已经接进二进制。下表跟踪当前 release 里哪些生效。

| Key                  | 状态        | 备注                                                            |
|----------------------|-------------|-----------------------------------------------------------------|
| `model.default`      | 已读取      | flag 缺失时通过 `--model` 暴露。                                |
| `model.audit_model`  | 预留        | 等审计派发接上 provider 后生效。                                |
| `provider.kind`      | 预留        | provider 目前由 CLI 二进制注入。                                |
| `isolation.strategy` | 已读取      | 映射到 `IsolationStrategy`（`Auto`/`Git`/`Mirror`/`Copy`）。    |
| `isolation.workspace_parent` | 已读取 | 传给 `WorkspaceManager::with_workspace_parent`。             |
| `cleanup.delay_days` | 已读取      | 默认 3 天。`0` 立即清理。                                        |
| `memory.max_active_tokens` | 已读取 | 封顶编排器的活跃区。                                            |
| `hooks.default_timeout_ms` | 预留   | `hooks.toml` 里 per-hook 的 `timeout_ms` 已经能用。            |
| `audit.risk_threshold` | 预留      | 风险分级在跑，但阈值是硬编码的。                                |

标 `预留` 的 key 记录的是约定的契约，让配置文件保持前向兼容。

## `.agentignore`

语法同 `.gitignore`。这里列出的 pattern 会被排除出工作区 mirror 导入，以及 `CopyWorkspace` 的目录遍历。把你的重型构建产物放这儿，工作区创建就能保持快。

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

两个额外行为值得知道：

- **目录符号链接。** 被排除但 agent 仍想读取的目录（比如 `node_modules/`、`target/`），会在工作区创建后从源项目符号链接过来，这样读取照常，又不必付拷贝代价。
- **二进制文件。** 二进制文件永不进入内部 git mirror，因为 `git diff` 对它们没意义，还会让 mirror 膨胀。只有文本文件参与 diff 和合并。

pattern 按正斜杠路径匹配，大小写敏感，与平台无关。空行和以 `#` 开头的行被忽略。取反（`!`）的语义同 `.gitignore`。

## 环境变量

| 变量                  | 用途                                                          |
|-----------------------|---------------------------------------------------------------|
| `ANTHROPIC_API_KEY`   | Anthropic Messages provider 的 API key。                      |
| `OPENAI_API_KEY`      | OpenAI Chat Completions provider 的 API key。                 |
| `GEMINI_API_KEY`      | Google Gemini provider 的 API key。                           |
| `OPCA_MODEL`          | 默认 model id。可被 `--model` 覆盖。                          |
| `OPCA_PROJECT`        | 不传 `--project` 时的默认项目路径。                           |
| `OPCA_WORKSPACE_PARENT` | 任务工作区的父目录。                                        |
| `RUST_LOG`            | 设置时覆盖 `-v` / `-vv` / `-vvv` 的过滤级别。                  |
| `NO_COLOR`            | 关闭日志输出的 ANSI 颜色。                                    |

API key 变量由 provider 构造函数（`AnthropicProvider::new`、`OpenAIProvider::new`、`GeminiProvider::new`）在 CLI 接线时读取。把它们放进 shell rc 文件，或通过 `.env` loader 设置。二进制本身不读 `.env` 文件。

```sh
# Example: export once per shell session
export ANTHROPIC_API_KEY="sk-ant-..."
opca --model claude-sonnet-4-20250514
```

## 工作区隔离策略

每个后台任务在自己的工作区里干活，并发任务互不踩脚。挑一个匹配你项目的策略。

### `auto`（默认）

- 项目有 `.git` 目录，则用 **git worktree**。
- 否则用**内部 git mirror**。
- mirror 创建失败则退化为**完整拷贝**。

95% 的情况你想要的就是它。

### `git-mirror`（非 git 项目）

在 `.agent/mirror/` 下创建一个全新 git 仓库，导入项目的文本文件（跳过 `.agentignore` pattern 和二进制文件），再为每个任务 check out 一个 worktree。文件系统支持时用 Copy-on-Write：

- macOS APFS，通过 `cp -R -c` 用 `clonefile`。
- Linux btrfs / xfs，通过 `cp -R --reflink=auto` 用 `reflink`。
- 其他文件系统，完整递归拷贝。

非 git 项目的合并流程：抽出 worktree 的 diff，把 patch 应用到原始项目目录，再刷新 mirror 基线。

### `copy`（兜底）

完整递归目录拷贝。最慢、磁盘占用最高，但哪儿都能跑，不依赖 git。git 不可用，或项目形状让 mirror 导入器犯迷糊时用它。

### `none`

不隔离。任务对真实项目目录串行执行。适合只读的检视工作，或者你足够信任单个任务，懒得搞工作区那套。这种模式下后台派发实际上是串行的。

### 挑选策略

```toml
# .agent/config.toml
[isolation]
strategy = "git-mirror"   # force mirror even for git projects
# strategy = "copy"       # safest, slowest
# strategy = "none"       # no isolation, serial Tasks
```

或按次调用覆盖：

```sh
opca --project .  # strategy comes from config or defaults to "auto"
```

## 会话与冷存储

`opca` 在 `.agent/` 下持久化三样东西：

| 文件                           | 装的内容                                          |
|--------------------------------|---------------------------------------------------|
| `sessions/<id>.jsonl`          | 单个会话的追加式条目流。                          |
| `session-index.sqlite`         | 每个 session 的元数据，用于"恢复"菜单。           |
| `cold-store.sqlite`            | 跨会话召回归档（长期记忆）。                      |

JSONL 可人工检视、可 git diff。SQLite 给元数据建索引，所以列出或恢复 session 时不必解析每个日志文件。冷存储跨会话存活：编排器归档进去的任何东西，在之后的会话里通过 `recall` 工具仍可召回。

恢复之前的 session：

```sh
opca --session 01HGE9R1K7QX...
```

省略 `--session` 则开新的。开启 verbose 日志（`-v`）时，session id 会在启动时打印出来。
