## Context

本项目从零构建一个 Rust 编写的 AI coding agent，核心卖点是**后台优先、前台永不阻塞**。用户派发任务后立即可以继续对话，任务在后台 worker 中执行，完成后通知用户。

调研了 pi_agent_rust（Dicklesworthstone/pi_agent_rust）的架构，结论是**不 fork**：
- 它绑定作者自研的 asupersync 运行时（非 tokio），社区小、文档少
- src/ 32 万行，agent.rs 单文件 312KB，大量 aspirational 桩代码（hostcall_io_uring_lane.rs 自承认"只建模策略决策，不执行 syscall"）
- 核心是串行 REPL（AgentSession 被 `Arc<Mutex>` 串行化），不支持本项目的核心需求（后台任务 + 前台继续聊）

调研了 6 家同类产品的扩展模型（Claude Code / Cursor / Cline / Continue / Windsurf / Aider），结论是**不做统一 plugin API，做三类分离扩展点**：
- 没有一家产品用统一的"插件"概念，都是分类扩展
- Claude Code 和 Cursor 的 "plugin" 本质是打包格式（bundle 已有 primitive）
- Hook 是唯一能让规则从"建议"变成"强制"的机制
- Cline 是唯一做代码级 SDK 的，但代价是限定 TypeScript + 子进程沙箱复杂

技术栈：Rust + tokio + 行式 CLI。开发方法：TDD。

## Goals / Non-Goals

**Goals:**

- 用户派发任务后立即可以继续对话，前台永不阻塞
- 多个后台 Task 并行执行，workspace 隔离（git worktree / 内部 git 镜像）
- Task 有拟人化生命周期，状态可见、可查询
- 主脑（Orchestrator）聚合所有 Task 状态，用户只需和主脑对话
- Task 主动上报重要发现（Focus Contract），主脑上下文不被淹没
- 主脑有三级记忆（Active / Archive / Cold Store），可 recall 历史信息
- Task 完成后由独立 Audit Agent 验收，风险分级处理
- 扩展体系三类分离（Context / Capability / Hook），Plugin 仅作打包
- 能力对标 Claude Code / OpenCode（工具调用、多模型、会话持久化、子 agent 编排）
- TDD 驱动，不确定性隔离在 Provider trait 后面

**Non-Goals:**

- 不 fork pi_agent_rust，不共享其代码或依赖
- 不做全屏 TUI（行式 CLI 优先，后续可升级双区域显示）
- 不做 WASM/JS 插件运行时（MCP 外部进程 + 外部命令 hook 足矣）
- 不做代码级插件 SDK（复杂逻辑由 Task/Audit 承担）
- 不做分布式/多租户（单机本地工具）
- 不做 IDE 插件（CLI 优先，后续可加 RPC 模式给 IDE 集成）
- 不追求与 pi_agent_rust 的 session 格式兼容

## Decisions

### D1: 异步运行时——tokio，不是 asupersync

**选择**：tokio
**理由**：生态最成熟、文档最全、AI 知识储备最丰富、长期维护有保障。asupersync 是单人维护的项目，fork pi_agent_rust 等于绑定到一个不可控的依赖。
**备选**：asupersync（pi_agent_rust 用），async-std（已不再活跃维护）。

### D2: 单进程多 task 架构（不是多进程）

**选择**：单进程，Orchestrator 和 Task 都是 tokio task（`tokio::spawn`），通过 channel 通信。
**理由**：
- Task 之间需要共享 Memory、Workspace 管理器等状态，进程间通信开销大
- 单进程内 channel 通信足够快
- Task 崩溃用 `tokio::task` 的 panic 捕获 + 状态恢复处理，不需要进程隔离

**架构**：

```
┌─ 进程内架构 ───────────────────────────────────────────────┐
│                                                            │
│  CLI 前台 (tokio task)                                     │
│    ├─ 输入循环: 永远可读 stdin                             │
│    └─ 输出区: Orchestrator 回复 + Task 完成通知            │
│         ↕ mpsc channel                                     │
│  Orchestrator (tokio task)                                 │
│    ├─ Active Memory (context window)                       │
│    ├─ Archive + Cold Store (SQLite + 索引)                 │
│    ├─ Task Registry (所有活跃 Task 的状态)                 │
│    └─ 工具: recall, deep_dive, dispatch, update_focus      │
│         ↕ per-task channel pair (steering + heartbeat)     │
│  Task A (tokio::spawn)     Task B (tokio::spawn)          │
│    ├─ Agent Loop            ├─ Agent Loop                  │
│    ├─ Memory<Message>       ├─ Memory<Message>             │
│    ├─ Workspace (worktree)  ├─ Workspace (worktree)        │
│    └─ Tool Registry         └─ Tool Registry               │
│         ↕                                                  │
│  Audit (tokio::spawn, 按需创建)                            │
│    └─ 只读访问 Task 的 diff + context                      │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

### D3: 非阻塞 CLI——静默后台模式（方案 A）

**选择**：静默后台 + 完成通知 + 主动查询。
**理由**：
- 实现最简单，纯行式，无终端控制序列，和管道/重定向兼容
- 覆盖 90% 的非阻塞体验
- 后续可升级到双区域显示（crossterm 光标控制）

**行为**：
- 后台 Task 默认不输出到终端
- Task 完成时输出通知（`🔔 Ag: A 搞定了,改了 5 个文件`）
- 用户可随时查询进度（`Ag: A 怎么样了?` → 主脑从 heartbeat 获取状态）
- Task 上报的 highlight 进入主脑 context，主脑在回复用户时自然提及

**备选**：双区域显示（crossterm，输出区 + 固定输入行），混合模式（TTY 检测）。

### D4: 拟人化生命周期状态机

**选择**：随性日常风格的命名。

```
💤 Sleeping   → 进程刚启动,还没加载完配置/插件
🌅 Waking     → 初始化 worktree,加载 context
🤔 Pondering  → 思考方案 (planning turn)
🔨 OnIt       → 干活中 (executing)
🫥 Waiting    → 遇到问题,等你或主脑回复
🔍 Reviewing  → 干完了,自检中 (或被 Audit 审计中)
✅ Delivered  → 完成,结果待验收
😵 Stuck      → 卡住了,需要帮助
✂️ Axed       → 被取消
📦 Archived   → 完整归档,worktree 已清理
```

**状态转换规则**：
- Sleeping → Waking（初始化）
- Waking → Pondering（第一个 turn）
- Pondering → OnIt（开始执行）
- OnIt → Pondering（下一个 turn）| Waiting（需要输入）| Delivered（完成）| Stuck（卡住）
- Waiting → OnIt（收到回复）| Axed（超时取消）
- Delivered → Reviewing（开始审计）| Archived（直接归档,低风险跳过审计）
- Reviewing → Archived（验收通过）| OnIt（验收拒绝,回炉）| Axed（废弃）
- Stuck → OnIt（获得帮助）| Axed（放弃）
- 任何状态 → Axed（用户/主脑取消）
- Axed → Archived（清理）

**状态转换时自动推送 heartbeat**——不需要额外机制。

### D5: 三层上下文 + Focus Contract

**选择**：每个 Task 维护三层上下文，主脑默认只看 Layer 1+2。

```
Layer 1 — Heartbeat (自动)
  生成: 每个 turn 结束 + 状态转换时
  内容: 状态 + 进度 + 当前在做的事 (一句话)
  大小: ~50 tokens
  推送: 自动推给 Orchestrator

Layer 2 — Highlights (主动上报)
  生成: Agent 调用 report_highlight 工具
  内容: 重要发现 / 阻塞 / 决策 / 需要人确认的事
  大小: ~100-300 tokens / 条
  推送: 标记后立即推给 Orchestrator

Layer 3 — Full (按需访问)
  生成: Agent loop 的原始消息流
  内容: 所有 messages + tool calls + tool results
  大小: 可能几万 tokens
  推送: ❌ 永远不主动推送
  访问: Orchestrator 通过 deep_dive 工具按需查片段
```

**Focus Contract**：
- 主脑派发 Task 时签订，指定 Task 应上报哪些维度
- 注入到 Task 的 system prompt："你必须关注以下维度...[安全风险, 破坏性变更, ...]"
- Task 的 report_highlight 工具参数 `tag` 必须匹配 focus list
- **动态调整**：主脑通过 steering 通道发送 update_focus（add/remove）
- **淘汰策略**：C + 上限兜底——主脑全权管理，硬上限 8 条，超了必须先 remove 再 add
- **分档**（可选扩展）：focus 分"必报"（实时推）和"汇总报"（turn 结束摘要推）两档

### D6: 记忆体系——分形 Memory&lt;T&gt;

**选择**：三级记忆，同一套 Memory 组件在 Task、Orchestrator、跨会话三层复用。

```
Memory<T> {
  active: Vec<T>              // 当前 context window
  archive: Store              // 持久化存储 (SQLite)
  index: MultiIndex           // 多维索引

  compact():                  // 窗口接近上限时
    将 active 旧内容压缩 → 存入 archive
    建索引 (keyword/time/tag/embedding)
    active 只保留最近 + 摘要

  recall(query) -> Vec<T>:    // 按需检索
    从 archive 按索引检索
    返回相关片段

  remember(item: T, tags: []):  // 主动存储
    存入 archive + 建索引
}
```

**三层分形**：
- Task 用 `Memory<Message>`（Active = loop 内 context，Archive = Layer 3 全量）
- Orchestrator 用 `Memory<ConversationEvent>`（Active = 主脑 context，Archive = 压缩历史）
- Cold Store 用 `Memory<SessionSummary>`（跨会话持久化）

**索引维度**：keyword（倒排）、time（时间范围）、task_id、tag（focus 标签）、semantic（embedding 向量检索）。

**Recall 触发**：主脑自己判断要不要 recall（LLM 自主决策），同时后台异步预取——用户说完话后跑一次关键词检索，命中则把摘要放进 context，不增加首响延迟。

**Compaction 策略**（Orchestrator 级）：
- 已完成 Task 的 highlights → 压成 1 条最终摘要
- 进行中 Task 的旧 highlights → 滚动压缩
- 心跳只保留最新一条（旧的丢弃）

### D7: Workspace 隔离——Workspace trait + 内部 git 镜像

**选择**：统一 Workspace trait，git 项目用原生 worktree，非 git 项目用内部 git 镜像。

```rust
trait Workspace {
    fn create(project, task_id) -> Workspace;
    fn path(&self) -> &Path;           // Agent 的工作路径
    fn freeze(&mut self);              // 冻结,禁止写
    fn cleanup(&mut self, delay);      // 延迟清理

    fn diff(&self) -> ChangeSet;       // 相对 baseline 的改动
    fn commit(&mut self, msg);         // 保存当前状态

    fn merge_into(&self, target: &mut Workspace)
        -> MergeResult;                // Clean | Conflict(files) | Failed
}
```

**三种实现**：
- `GitWorkspace`：git 项目，直接 `git worktree add`
- `MirrorWorkspace`：非 git 项目，在 `.agent/mirror/` 建内部 git repo，导入项目文件（CoW 加速：APFS clonefile / btrfs reflink），再 `git worktree add`
- `CopyWorkspace`：降级，完整目录复制（磁盘开销大，最后手段）

**非 git 合并流程**：提取 worktree diff → patch 应用到原始项目目录 → 刷新 mirror baseline。

**.agentignore**：排除 node_modules/、target/、dist/ 等大目录。worktree 创建后 symlink 这些目录到主目录（读共享，不追踪改动）。

**大文件处理**：二进制文件不导入 mirror，git diff 对二进制无意义。

**配置项**：`isolation = "auto" | "git-mirror" | "copy" | "none"`。`none` = 不隔离，串行执行（降级体验）。

### D8: 审计 Agent——Task 的只读特化变体

**选择**：Audit 是 Agent role=Auditor 的特化（B 方案），不是独立顶层概念。
**理由**：复用 Task 的 Agent 抽象、Memory、Focus Contract、highlight 推送。只需配置区分（role=Auditor, readonly=true, lifecycle=spawn_die）。

**生命周期**（极简）：Spawn → Inspect → Judge → Report → Die。没有 Sleeping/Waking/Pondering 等。

**验收模式**：C+D 混合（分级 + 协作）。
- 低风险（diff < 20 行 + 只改 .md 等）→ 规则检查（编译/测试），跳过 Audit
- 中高风险 → 主脑派 Audit Agent（带 focus）

**Audit Focus 来源**：
- 继承自 Task focus（Task 关注的维度，Audit 也要查）
- 标准检查项（编译通过？测试通过？diff 合理？）
- 主脑额外指定

**审计模式**：默认黑盒（只看 diff + final summary + 跑测试），按需 deep dive（Audit 有 `deep_dive_task_context` 工具，自己判断要不要深入看 Task 推理过程）。

**Audit Report**：
```json
{
  "verdict": "pass" | "warn" | "fail",
  "confidence": 0.0-1.0,
  "findings": [
    { "severity": "critical|minor", "location": "auth.rs:42", "issue": "..." }
  ],
  "summary": "整体 OK,但 token 刷新逻辑有个边界条件没处理"
}
```

**权力链**：Audit（顾问，检查）→ 主脑（决策者，可推翻 Audit）→ 用户（高风险终审）。

**成本控制**：Audit 用便宜模型（haiku/flash）；只跑 1-3 turn（不是完整 loop）；低风险跳过 Audit。

### D9: 完成后链路——5 阶段

```
① Freeze    worktree 冻结(只读),生成 final summary,heartbeat "✅Delivered",通知主脑+CLI
② Review    风险评估 → 低风险规则检查 / 中高风险派 Audit → 主脑决策(自动/转发用户)
③ Merge     冲突检测(预防为主:派发时预测交集) → 无冲突直接合并 / 有冲突主脑 auto-resolve / 失败问用户
④ Memorialize  final summary → 主脑 Active Memory; 全量 context+diff → Cold Store(带索引); 主脑 compaction
⑤ Cleanup   worktree 延迟 N 天清理; Cold Store 保留(记忆不丢)
```

**冲突预防**：主脑派发前做轻量冲突预测——预估每个 Task 会碰哪些文件，有交集则串行化。

**完成通知（边界情况）**：用户正在忙时，低风险自动处理（不打扰），高风险标记"待验收"排队（CLI 提示栏显示数量，不弹窗）。

**依赖链**：Task A 完成+合并后自动激活依赖 A 的 Task B（B 的 worktree 基于合并后的 main 重新创建）。

**worktree 清理**：合并后保留 N 天（默认 3 天，可配），然后 `git worktree remove`。Cold Store 里的全量 context 不受影响。

### D10: 扩展体系——三类分离 + Plugin 打包

**选择**：不做统一 plugin API，做三个明确分离的扩展点。

```
① Context (上下文注入)
   形态: AGENTS.md + skills/*.md (纯 Markdown)
   做什么: 教 agent "怎么思考和行动"
   实现: 纯文本,注入 system prompt

② Capability (能力供给)
   形态: MCP server (外部进程, JSON-RPC over stdin/stdout)
   做什么: 提供新工具 (查数据库/调 API)
   实现: 进程隔离, 语言无关
   红利: 兼容 MCP 生态, 几百个现成 server

③ Hook (生命周期拦截)
   形态: 外部命令/脚本, 在生命周期固定点触发
   做什么: 确定性强制 (提交前测试/危险命令阻断)
   实现: 事件 → matcher → 外部进程, stdin JSON / stdout JSON
   可阻断: on_pre_tool_use, on_merge_pre 返回 deny 可阻止操作
```

**Plugin = 打包格式**（借鉴 Claude Code / Cursor）：
```
my-plugin/
  plugin.toml          # 清单: name, version, author
  AGENTS.md            # Context 注入
  skills/              # Skill 文件
  mcp.json             # MCP server 配置
  hooks.toml           # Hook 定义
```

Plugin 没有引入新能力，只是"一键安装一组扩展"。用户也可以不用 plugin，手动配置三个扩展点。

**Hook 事件覆盖**（四级）：
- Session: on_session_start, on_session_end
- Orchestrator: on_user_message, on_pre_dispatch, on_post_dispatch, on_task_highlight, on_recall, on_merge_pre(⭐可阻断), on_merge_post
- Task: on_task_create, on_task_freeze, on_task_reject, on_task_archive, on_pre_tool_use(⭐可阻断), on_post_tool_use
- Audit: on_audit_start, on_audit_report, on_audit_override

**Hook handler 类型**（借鉴 Claude Code）：command（shell）、http（POST JSON）、mcp_tool（调 MCP 工具）、prompt（LLM 单轮判断）、agent（spawn subagent 验证）。

**工具激活策略**：D 方案——插件声明适用范围（keywords/task_types），主脑最终决定每个 Task 激活哪些插件。不给 Task 不需要的权限，减少 token 注入和选择困难。

**Provider 插件**：允许插件通过 HTTP 代理模式提供新 LLM 接入（插件 = 本地 HTTP server + LLM 代理，框架用 HTTP/SSE 通信）。

**不做 WASM/JS 运行时**——pi_agent_rust 走了 WASM+JS+native 三套是反面教材。工具层 90% 场景被声明式配置 + MCP 覆盖，Hook 层用外部命令足够。

### D11: TDD——三层策略 + Provider trait 支点

**选择**：TDD 驱动，不确定性隔离在 Provider trait 后面。

```
Layer 1: 基础设施 (60-70% 代码量, 完全确定性)
  Memory<T>, Workspace trait, Lifecycle 状态机,
  Focus Contract, Hook 系统, Tool Registry, Plugin Loader
  → 标准 TDD 循环 (Red → Green → Refactor)
  → 纯 Rust 单元测试, in-memory SQLite, tempfile

Layer 2: 编排逻辑 (中等确定性)
  Task Agent Loop, Orchestrator 路由, 审计验收
  → ScriptedProvider (预编程响应序列) + MockWorkspace
  → 测 "给定 LLM 说了X,系统怎么响应"

Layer 3: 真实接入 (不确定性)
  Anthropic/OpenAI Provider, 真实 git worktree
  → 少量 E2E smoke test + 手动验收
  → 不做 TDD (LLM 不确定性太大)
```

**Provider trait = 可测试性支点**：
```rust
trait Provider {
    async fn stream(&self, messages: &[Message], tools: &[ToolDef])
        -> Stream<Event>;
}

// 生产: AnthropicProvider, OpenAIProvider, ...
// 测试: ScriptedProvider (then_tool_call(...).then_text(...).then_done())
```

**依赖注入 trait 体系**：FileSystem, Process, Clock, Random——所有外部依赖通过 trait 注入。

**关键测试工具**：
- `rstest`（参数化测试）
- `insta`（快照测试——heartbeat 格式、highlight 结构、prompt 模板）
- `proptest`（属性测试——状态机转换合法性、Memory compact 不丢数据）
- `mockall`（mock 生成）
- `tempfile`（临时目录，自动清理）
- `wiremock`（HTTP mock，测 Provider 实现）

**开发顺序（TDD 驱动）**：
1. Phase 1: 地基（Memory, Workspace, Lifecycle, Focus）——纯确定性，标准 TDD
2. Phase 2: 编排层（Provider trait + ScriptedProvider, Task Loop, Orchestrator, Audit）——fake provider
3. Phase 3: 真实接入（Anthropic/OpenAI Provider, 真实 git worktree, E2E）——少量 smoke test

### D12: 从 pi_agent_rust 借鉴的三个设计

- **ToolEffects 并行分类**：Tool 声明 effects（read/write/append/process），框架自动决定哪些 tool 可并行（read 类并行，write 类串行）。比手动标注每个 tool 优雅。
- **steering + follow-up 双队列**：Task 进行中可通过 steering 队列注入新指令（如 update_focus），idle 时的消息进 follow-up 队列。这是非阻塞体验的底层机制。
- **Cow<'_, [Message]> 零拷贝 context 构建**：构建 LLM context 时用 Cow 引用而非 clone，大 session 下性能显著。

## Risks / Trade-offs

### [Risk] 上下文层化的信息丢失
Task 的 Layer 3 全量上下文默认不被主脑看到，可能遗漏重要信息。
→ **Mitigation**：Focus Contract 确保关键维度被上报；主脑有 deep_dive 工具按需访问；Audit 在验收时可白盒检查 Task 推理过程。

### [Risk] Focus Contract 膨胀
主脑不断 add focus，Task 上报越来越多，主脑 context 被淹没。
→ **Mitigation**：C + 上限兜底——主脑全权管理，硬上限 8 条，超了必须先 remove 再 add。

### [Risk] 非 git 镜像的首次导入慢
大项目首次导入 mirror 耗时，用户体验差。
→ **Mitigation**：CoW 加速（APFS clonefile / btrfs reflink）；.agentignore 排除大目录；symlink node_modules/target 等到主目录；首次初始化时显示进度。

### [Risk] 并发 Task 的 merge 冲突
多个 Task 同时完成且改了相同文件。
→ **Mitigation**：预防为主——派发时做冲突预测（预估每个 Task 会碰哪些文件，有交集则串行化）；治疗后备——主脑尝试 auto-resolve，失败则问用户。

### [Risk] Audit 误判
Audit Agent 可能误判（假阳性报 fail，或假阴性放过 bug）。
→ **Mitigation**：主脑可推翻 Audit 结论（主脑是决策者，Audit 是顾问）；高风险任务必须人工终审；Audit confidence 分数 < 阈值时升级到人工。

### [Risk] tokio task panic 导致 Task 崩溃
单个 Task 的 panic 可能影响整个进程。
→ **Mitigation**：`tokio::spawn` 返回的 JoinHandle 用 `await` 捕获 panic；Task 崩溃后状态转为 💀 Crashed（→ Archived），通知主脑和用户；Memory 和 worktree 状态持久化，可恢复。

### [Risk] Hook 外部进程的性能开销
每次 on_pre_tool_use 都 spawn 进程，可能拖慢 Agent loop。
→ **Mitigation**：Hook 有 timeout（默认 10s）；async 模式（asyncRewake，借鉴 Claude Code）；matcher 过滤减少不必要的 hook 触发；热路径（on_post_tool_use）可配置为 fire-and-forget。

### [Trade-off] 静默后台模式缺少即时反馈
方案 A（静默）不如双区域显示（方案 B）那样能实时看到 Task 进度。
→ **接受**：先做 A（实现成本低一个量级），体验足够后再升级 B。用户可主动查询进度作为补偿。

### [Trade-off] 不做代码级插件 SDK 限制了插件表达力
相比 Cline 的 `setup(api)` + `registerTool`，本项目的插件只能通过 MCP 供给工具，不能在进程内跑代码。
→ **接受**：复杂逻辑由 Task/Audit 承担，插件只需供给和拦截。MCP 生态已有几百个现成 server。

### [Trade-off] Audit 用便宜模型可能不够准确
haiku/flash 级别的模型做审计判断，可能漏掉复杂问题。
→ **接受 + 可调**：默认便宜模型（成本控制），高风险任务用强模型。用户可配置 `audit_model` 覆盖。

## Migration Plan

不适用——这是全新项目，无迁移。

## Open Questions

1. **semantic embedding 索引用什么模型？** 本地（如 candle + all-MiniLM）还是 API（OpenAI embedding）？影响 Cold Store 的离线可用性。
2. **worktree 清理的 N 天默认值？** 暂定 3 天，需用户反馈调整。
3. **CLI 输入编辑器用什么？** `reedline`（nushell 用的，功能全）还是 `rustyline`（轻量）还是自写？
4. **会话持久化格式？** JSONL（pi_agent_rust 方案）还是直接 SQLite？JSONL 便于 git diff 和人工检查，SQLite 查询快。暂定 JSONL 主 + SQLite 索引（双层）。
5. **MCP 实现用 rmcp crate 还是自己写？** rmcp 是 Rust MCP SDK，需评估成熟度。
