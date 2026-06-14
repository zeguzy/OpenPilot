## Why

现存的 AI coding agent（Claude Code、OpenCode、Cursor 等）普遍采用串行 REPL 模型——用户提交任务后必须等待 agent 完成当前 turn 才能继续交互。这与真实工程工作中"派活后继续干别的"的协作模式冲突。我们需要一个**后台优先**的 agent：所有耗时任务派发到后台 worker，前台永不阻塞，用户随时可以发新消息、查询进度、或同时管理多个任务。

本提案同时定义了项目的**初始构建计划**——这是一个全新项目，从零开始，借鉴 pi_agent_rust 的若干设计思路但不 fork 其代码库。

## What Changes

### 核心架构变更（全新项目）

- **新增：多 agent 并发架构**——由一个常驻 Orchestrator（主脑）管理多个后台 Task（子 agent），前台 CLI 通过 Orchestrator 与 Task 交互。Orchestrator 决定路由（前台快回复 vs 后台长任务），派发 Task，聚合状态，与用户对话。**这是非阻塞体验的根基。**
- **新增：拟人化生命周期状态机**——Task 有活泼的状态名（💤 Sleeping / 🌅 Waking / 🤔 Pondering / 🔨 OnIt / 🫥 Waiting / 🔍 Reviewing / ✅ Delivered / 😵 Stuck / ✂️ Axed / 📦 Archived）。状态转换时自动推送 heartbeat。
- **新增：上下文分层模型**——每个 Task 维护三层上下文：Layer 1 Heartbeat（自动心跳，~50 tokens）、Layer 2 Highlights（主动上报的关键发现，~100-300 tokens）、Layer 3 Full（完整消息历史，按需访问）。Orchestrator 默认只看 Layer 1+2，按需 deep dive Layer 3。
- **新增：Focus Contract**——主脑派发 Task 时签订"关注契约"，指定 Task 应上报哪些维度（如"安全风险"、"破坏性变更"）。契约可动态调整（add/remove），上限 8 条，主脑全权管理。
- **新增：Task 间 workspace 隔离**——每个后台 Task 在独立的 git worktree（git 项目）或内部 git 镜像（非 git 项目）中工作，操作互不影响。
- **新增：独立 Audit Agent**——Task 完成后由专门的审计 agent 验收（Task 的只读特化变体）。默认黑盒审计（只看 diff + 跑测试），按需 deep dive Task 全量上下文。主脑依据 Audit 报告做风险分级决策（低风险自动合并 / 高风险人工终审）。
- **新增：三级记忆体系**——Active（当前 context window）、Archive（压缩后的历史 + 多维索引：keyword/time/task/tag/semantic）、Cold Store（跨会话持久化）。分形复用——Task 和 Orchestrator 各自维护自己的 Memory 实例。
- **新增：扩展体系（三类分离 + Plugin 打包）**——Context（AGENTS.md + skills/*.md 纯 Markdown）、Capability（MCP server 外部进程）、Hook（生命周期事件拦截，可阻断，借鉴 Claude Code 的 prompt/agent handler 类型）。Plugin 是分发打包格式，不引入新机制。
- **新增：完整生命周期 Hook 系统**——覆盖 Session / Orchestrator / Task / Audit 四级事件（on_session_start, on_pre_dispatch, on_task_freeze, on_audit_start, on_merge_pre 等）。`on_pre_tool_use` 和 `on_merge_pre` 等 hook 可阻断操作。
- **新增：TDD 驱动开发**——三层测试策略：Layer 1 基础设施（纯单元测试，标准 TDD 循环）、Layer 2 编排逻辑（ScriptedProvider mock + mock workspace）、Layer 3 真实接入（少量 E2E smoke test）。Provider trait 是可测试性的支点——不确定性被隔离在 trait 后面。

### 借鉴但不照搬 pi_agent_rust 的设计

- ToolEffects 并行分类（read/write/process → 自动决定 tool 是否可并行）
- steering + follow-up 双队列（任务进行中可注入新指令）
- Cow<'_, [Message]> 零拷贝 context 构建

### 明确不做的

- **不 fork pi_agent_rust**——它绑定作者自研的 asupersync 运行时（非 tokio），src/ 32 万行大量是 aspirational 桩代码（hostcall_io_uring_lane 自承认不干活）。本项目用 tokio，从零开始。
- **不做全屏 TUI**——行式 CLI 为主。先做"静默后台 + 完成通知 + 主动查询"方案，体验足够后再考虑双区域显示。
- **不做 WASM/JS 插件运行时**——pi_agent_rust 走了 WASM+JS+native 三套是反面教材。插件只走 MCP 外部进程 + 外部命令 hook。
- **不做代码级插件 SDK**——复杂逻辑由 Task/Audit 承担，插件只负责供给（Context + Capability）和拦截（Hook）。

## Capabilities

### New Capabilities

- `orchestrator-core`: 常驻主脑 agent，承担路由决策、Task 派发与 Focus Contract 签订、状态聚合、与用户对话。包含 Active Memory 管理、recall 检索、conflict 预测。
- `task-lifecycle`: 拟人化生命周期状态机（Sleeping → Waking → Pondering → OnIt → Waiting → Reviewing → Delivered → Stuck → Axed → Archived），状态转换触发 heartbeat 推送。
- `task-context-layering`: 三层上下文模型（Heartbeat / Highlights / Full）+ Focus Contract（动态关注契约，上限 8 条）+ report_highlight 工具。
- `workspace-isolation`: Workspace trait 抽象（Git worktree / 内部 git 镜像 / 完整复制），git 与非 git 项目统一隔离。含 create/freeze/diff/merge/cleanup 全生命周期。
- `audit-agent`: 独立审计 agent（Task 的只读特化变体），黑盒审计 + 按需 deep dive，verdict 报告（pass/warn/fail），主脑可推翻结论。
- `memory-system`: 分形 Memory&lt;T&gt; 组件（Active + Archive + Cold Store），compact/recall/remember 操作，多维索引（keyword/time/task/tag/semantic embedding）。
- `completion-pipeline`: Task 完成后的完整收尾链路：Freeze → 风险评估 → Review（分级：低风险自动 / 高风险协作）→ Merge（冲突检测 + 主脑 auto-resolve）→ Memorialize（归档进 Cold Store）→ Cleanup（延迟清理 worktree）。
- `extension-system`: 三类分离的扩展点（Context = Markdown、Capability = MCP server、Hook = 生命周期命令）+ Plugin 打包格式（toml 清单 bundle 三者）。含 MCP 兼容（复用生态）、Hook 事件系统（Session/Orchestrator/Task/Audit 四级，可阻断）、prompt/agent handler 类型（LLM-as-hook-judge）。
- `provider-abstraction`: Provider trait + ScriptedProvider（测试用）+ 多 provider 实现（Anthropic/OpenAI/Gemini 等）。流式 SSE，零拷贝 context 构建。这是 TDD 可测试性的支点。
- `cli-frontend`: 行式 CLI 前台，永远可输入。静默后台模式：后台 Task 完成时通知、用户可主动查询进度。输入路由到 Orchestrator。
- `tdd-foundation`: 测试基础设施——ScriptedProvider、MockWorkspace、FakeClock、in-memory SQLite、insta 快照测试。依赖注入 trait 体系（FileSystem/Process/Clock/Random）。

### Modified Capabilities

无——这是全新项目，openspec/specs/ 为空。

## Impact

### 代码

- 全新 Rust 项目，从零构建
- 预计模块：`orchestrator/`, `task/`, `audit/`, `memory/`, `workspace/`, `provider/`, `tools/`, `hooks/`, `extensions/`, `cli/`, `lifecycle/`
- Cargo workspace 多 crate 结构（核心 lib + cli binary + 测试工具 crate）

### 依赖

- `tokio`（异步运行时，**不是** asupersync）
- `clap`（CLI 解析）
- `rusqlite` 或 `sqlx`（SQLite，会话/记忆索引）
- `reqwest` + `eventsource-stream`（LLM API SSE 流式）
- `serde` / `serde_json`（消息序列化）
- `git2`（libgit2 绑定，worktree 管理）
- `tracing`（结构化日志）
- 测试：`rstest`, `proptest`, `insta`, `mockall`, `tempfile`, `wiremock`

### 系统

- 需要 git 可执行文件（worktree 操作；非 git 项目用内部镜像）
- 可选：CoW 文件系统支持（APFS clonefile / btrfs reflink）加速非 git 镜像创建
- macOS / Linux 优先；Windows 后续支持

### 与 pi_agent_rust 的关系

- **不 fork**，借鉴三个设计点（ToolEffects、steering/followup、零拷贝 context）
- 不共享代码，不共享依赖（pi 用 asupersync，本项目用 tokio）
