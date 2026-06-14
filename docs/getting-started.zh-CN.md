[English](getting-started.md) | [中文](getting-started.zh-CN.md)

# 入门指南

这份指南带你走完安装 opca、配置，以及跑通第一次会话的全过程。读完之后，你将能派发一个后台任务，在聊天的同时查询它的进度，并最终接受成果。

如果你想先看项目定位，读 [README](../README.zh-CN.md)。想要完整的工程拆解，读[架构文档](architecture.zh-CN.md)。

## 前置要求

开始之前，机器上需要三样东西。

- **Rust 1.85 或更高。** opca 用 edition 2024。用 `rustc --version` 检查。需要的话通过 [rustup](https://rustup.rs/) 安装或更新。
- **git。** git 项目用它做 worktree 隔离，非 git 项目用它做内部 mirror。用 `git --version` 验证。
- **一家受支持 provider 的 API key。** 先挑一家即可。

| Provider  | 环境变量               | 示例 model id                  |
|-----------|------------------------|--------------------------------|
| Anthropic | `ANTHROPIC_API_KEY`    | `claude-sonnet-4-20250514`     |
| OpenAI    | `OPENAI_API_KEY`       | `gpt-4o`                       |
| Gemini    | `GEMINI_API_KEY`       | `gemini-1.5-pro`               |

只需要一家。provider 由 CLI 注入，所以你启动时选定即可。

## 安装

opca 还没上 crates.io。请从源码构建。

```sh
git clone https://github.com/vhyc/openpilot-agent
cd openpilot-agent
cargo build --release
```

release 产物会落在 `target/release/opca-cli`。想直接用 `opca` 调用的话，把它拷到 `PATH` 上：

```sh
cp target/release/opca-cli ~/.local/bin/opca
```

验证它能跑：

```sh
opca --help
```

debug 构建用来尝鲜没问题，但真正跑长会话时 release 模式会明显更跟手。

## 配置

opca 按以下优先级读取三层配置：CLI flag（最高）、环境变量，然后是项目根目录下的 `.agent/config.toml`。完整参考见 [configuration.md](configuration.zh-CN.md)。这一节只讲够你起步的最小配置。

### 设置 API key

在 shell 里 export 它。把这行加进 shell rc 文件（`~/.zshrc`、`~/.bashrc`），让它持久生效。

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
```

二进制本身不读 `.env` 文件。如果你喜欢用 dotenv loader，请在启动 opca 之前自己跑一遍。

### 可选：项目配置

在想用 opca 的项目里创建 `.agent/config.toml`。每个 key 都是可选的。

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

这些对首次运行够用了。默认值很合理，所以你也可以完全不写这个文件，全部通过 flag 传。

## 第一次会话

在一个项目目录里启动 opca。指向项目和某个 model。

```sh
opca --project . --model claude-sonnet-4-20250514
```

你会看到 banner，然后是 prompt。REPL 永远准备好接收输入。前台没有任何东西会阻塞。

### 问一个简单问题

打一句不需要改文件的问题。

```
> what does the memory module do?
```

编排器直接从自己的活跃上下文作答。不会派发任何任务。这是快路径：消息进，答复出。

### 派发一个后台任务

现在让它干点真活儿。

```
> refactor the memory module to split store.rs into two files
```

编排器把它路由到后台。它签一份聚焦契约，创建一个隔离的工作区，然后 spawn 一个任务。你会看到这样一行：

```
🔨 OnIt: task-0 is working on the refactor
```

输出到此为止。任务从这里开始静默运行。你尽可以继续打字。

### 在任务跑的同时继续聊天

opca 的全部意义就在于前台归你。task-0 在那边啃重构的同时，问点别的。

```
> remind me, how does the lifecycle heartbeat work?
```

编排器立刻作答。后台任务不受影响。

### 查询进度

随时查看你的任务，自然语言或斜杠命令都行。

```
> how is task-0 going?
```

编排器从任务注册表里拉取最新心跳，告诉你当前状态、任务此刻在干什么，以及它汇报过的任何亮点。

结构化等价写法：

```
> /status task-0
```

一次列出所有活动任务：

```
> /tasks
```

### 处理完成的任务

任务完工时，你会收到一条完成通知。

```
🔔 task-0 finished: refactored store.rs into store.rs and index_impl.rs
   4 files changed, 312 insertions(+), 89 deletions(-)
   Audit verdict: warn (confidence 0.82)
```

低风险的活儿可能自动合并。中高风险的工作会等你定夺。先在工作区里看 diff，然后接受或拒绝。

接受成果：

```
> /accept task-0
```

任务合并进你的项目，编排器把完整上下文归档进冷存储，工作区则安排清理。

拒绝它，可以带上反馈，让它回到 `OnIt`：

```
> /reject task-0 "keep the index logic inside store.rs"
```

带反馈的话，任务会醒来，读一遍备注，重新尝试。不带反馈的话，它会被砍掉并归档。

## 斜杠命令

| 命令                      | 作用                                                  |
|---------------------------|-------------------------------------------------------|
| `/tasks`                  | 列出所有活动任务及其当前状态。                        |
| `/status [task-id]`       | 展开某一个任务详情，不带 id 则给出总览。              |
| `/accept <task-id>`       | 接受并合并一个已交付的任务。                          |
| `/reject <task-id> [msg]` | 拒绝一个任务。带消息时会回到 OnIt。                   |
| `/help`                   | 显示帮助文本。                                        |
| `/quit`                   | 退出 REPL。                                           |

这些不用死记。自然语言对除接受和拒绝之外的所有操作都管用，因为后两者需要显式的 task id。你问一句 "what's running?"，编排器就能领会。

## 接下来

- **[配置](configuration.zh-CN.md)** 调节 model、隔离策略、记忆上限、清理延迟，以及审计阈值。想跳出默认值时读它。
- **[插件编写指南](plugins.zh-CN.md)** 展示如何把 Context、Capability、Hook 三种扩展打包成一个可分发的插件。
- **[架构](architecture.zh-CN.md)** 讲解三角色模型、生命周期状态机、三层上下文，以及每个子系统背后的设计取舍。
