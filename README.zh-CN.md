[English](README.md) | [中文](README.zh-CN.md)

# opca

**Rust 编写的后台优先代码 agent。**

把耗时的工作派发到后台 worker，前台继续聊天，再也不用干等一个 agent 回合。编排器 (Orchestrator) 负责路由消息，多个任务 (Task) 在彼此隔离的工作区 (workspace) 里并行执行，独立的审计 (audit) agent 会在成果合并前做一次校验。

大多数代码 agent 跑的是串行 REPL。你提交一个 prompt，然后干坐着等它 chew 完几分钟的工具调用。opca 颠覆了这个模型。简单问题由编排器立刻作答；看着会慢的活儿会被派发到一个后台任务里，它有自己的工作区、自己的上下文窗口，还有一个随时可查询的拟人化生命周期 (lifecycle)。你照常聊天。任务完成时，你会收到一个通知、一份 diff，以及来自独立审计 agent 的裁定。

支撑这套体验的有三样东西：

- 编排器和每个任务之间签一份**聚焦契约 (Focus Contract)**，让主脑只看见关键信息，而不是被完整日志淹没。
- 每个任务都有**三层上下文**（心跳、亮点、完整历史），编排器按需通过 `deep_dive` 拉取细节。
- 通过 git worktree、内部 git mirror 或纯目录拷贝实现**工作区隔离**，让并行任务互不踩脚。

## 快速开始

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

OpenAI 和 Gemini 也能用。设置 `OPENAI_API_KEY` 或 `GEMINI_API_KEY`，并传入匹配的 model id。完整的 key 和 model 清单，以及 `.agent/config.toml` 的选项，见[配置文档](docs/configuration.zh-CN.md)。

想要一次引导式上手，读[入门指南](docs/getting-started.zh-CN.md)。

## 核心特性

- 🔨 **后台优先。** 每个长任务都在后台跑。前台 REPL 永不阻塞。你可以派发三个任务，然后问一个简单问题，在它们任何一个完成前就能拿到答案。
- 🧠 **三角色架构。** 编排器负责路由和决策，任务负责干活，审计 agent 负责校验结果。每个角色都有自己的上下文窗口和 provider。
- 💤 **拟人化生命周期。** 任务会经历一组带名字的状态：Sleeping、Waking、Pondering、OnIt、Waiting、Reviewing、Delivered、Stuck、Axed、Archived。每次状态迁移都会自动推送一条心跳，让你时刻清楚任务在干什么。
- 📊 **带聚焦契约的三层上下文。** 每个任务维护一条心跳（约 50 tokens）、若干亮点（约 100 到 300 tokens），以及一份完整历史。编排器签订聚焦契约，声明要汇报哪些维度，只在需要细节时才拉取完整流。
- 🗂️ **分形记忆。** 同一种 `Memory<T>` 结构在每一层复用：活跃上下文、压缩到 SQLite 的归档，以及跨会话的冷存储 (cold store)。`recall` 可以按关键词、时间、任务或标签检索。
- 🔒 **工作区隔离。** Git 项目用原生 worktree。非 git 项目用内部 git mirror，在 APFS、btrfs、xfs 上有 Copy-on-Write 加速。全量拷贝是兜底方案。
- 🔍 **独立审计 agent。** 任务的一个只读特化版本。它读 diff、跑测试，返回 pass、warn 或 fail 的裁定，并附带置信度分数。编排器可以否决它。
- 🔌 **三个扩展点。** Context（注入 prompt 的 Markdown）、Capability（以子进程方式跑 MCP server）、Hook（能阻断操作的生命周期拦截）。插件 (plugin) 把这三者打包到一起方便分发。
- ⚡ **TDD 驱动。** 三层加起来超过 590 个测试。在 `pedantic` 和 `nursery` 下零 clippy 警告。`Provider` trait 是把 LLM 的不确定性挡在单测之外的关键支点。
- 🦀 **从底到顶全是 Rust。** 在 workspace 层面禁止 `unsafe_code`。单进程、tokio 任务、channel 通信。没有 GC 停顿，库代码里没有运行时 panic。

## 架构

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

三个角色在同一个进程里协作。编排器是用户唯一交谈的对象。它路由消息、派发任务、汇聚心跳，并决定完工的成果是合并、送审还是交给用户定夺。

每个任务都住在自己的工作区里，拥有自己的 provider、记忆和工具注册表。任务通过三层向上汇报：自动的心跳、按需的亮点，以及编排器通过 `deep_dive` 才会拉取的完整消息历史。

审计 agent 是一个只读的任务。它按需被 spawn，读 diff、跑测试、给出裁定然后退出。默认用便宜模型来控制成本。

完整的工程拆解，包括状态机、完工流水线，以及每个子系统背后的设计取舍，见[架构文档](docs/architecture.zh-CN.md)。

## 项目结构

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

一个模块只管一件事。超过 400 行的文件会被拆开。类型的规范路径是 `opca_core::<area>::<Type>`。

## 配置

opca 按以下优先级读取三层配置：CLI flag、环境变量，然后是项目根目录下的 `.agent/config.toml`。配置文件可以控制默认 model、审计 model、工作区隔离策略、清理延迟、记忆上限，以及审计风险阈值。

完整参考：[docs/configuration.zh-CN.md](docs/configuration.zh-CN.md)。

## 插件

插件把三个扩展点（Context、Capability、Hook）打包到一个 `plugin.toml` manifest 后面。你可以用 Markdown 教会 agent 项目约定，通过 MCP server 暴露新工具，再用 hook 拦住不安全的操作。

编写指南：[docs/plugins.zh-CN.md](docs/plugins.zh-CN.md)。

## 开发

```sh
cargo build                              # debug build
cargo test --workspace                   # run all tests
cargo clippy --workspace --all-targets   # lint, must be clean
cargo fmt --all -- --check               # verify formatting
```

MSRV 是 **1.85**，edition **2024**。`unsafe_code` 在 workspace 层面被禁止。Clippy 跑 `pedantic` 加 `nursery`，allowlist 很短，调在 `Cargo.toml` 里。测试分三层：基础设施的纯单测、编排逻辑用的 `ScriptedProvider` 加 `MockWorkspace`，以及对真实 provider 调用加门槛的 smoke 测试。

项目对自己约定的要求，包括 TDD 工作流、commit 规则和模块布局，见 [AGENTS.md](AGENTS.md)。

## License

MIT
