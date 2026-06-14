[English](architecture.md) | [中文](architecture.zh-CN.md)

# 架构

这份文档讲的是 opca 怎么拼起来。它是 [README](../README.zh-CN.md) 那套定位背后的工程参考：进程模型、三个角色、状态机，以及塑造每个子系统的设计取舍。

日常使用和安装看 [getting-started.md](getting-started.zh-CN.md)。运行时旋钮看 [configuration.md](configuration.zh-CN.md)。

## 总览

opca 在一个进程里跑三个角色。

```
┌─ single process, tokio runtime ────────────────────────────┐
│                                                            │
│  CLI foreground (tokio task)                               │
│    input loop: always reading stdin                        │
│    output: Orchestrator replies + Task completion notices  │
│         ↕ mpsc channel                                     │
│  Orchestrator (tokio task)                                 │
│    Active Memory (context window)                          │
│    Archive + Cold Store (SQLite indices)                   │
│    Task Registry (state of every active Task)              │
│    Tools: recall, deep_dive, dispatch, update_focus        │
│         ↕ per-task channel pair (steering + heartbeat)     │
│  Task A (tokio::spawn)        Task B (tokio::spawn)        │
│    Agent Loop                   Agent Loop                 │
│    Memory<Message>             Memory<Message>             │
│    Workspace (worktree)        Workspace (worktree)        │
│    Tool Registry               Tool Registry               │
│         ↕                                                  │
│  Audit (tokio::spawn, on demand)                           │
│    read-only diff + test runs + verdict                    │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

- **编排器。** 每个会话一个。用户唯一交谈的对象。路由消息（快答复 vs 后台干活）、派发任务、汇聚心跳、管理聚焦契约、从冷存储里召回，并决定每个完工任务该合并、送审，还是交给用户定夺。
- **任务。** 一个后台 worker。拥有自己的 provider、工作区、`Memory<Message>`、聚焦契约和工具注册表。通过三层向上汇报：自动的心跳、按需的亮点，以及编排器按需拉取的完整消息历史。
- **审计。** 任务的一个只读特化版本（`role=Auditor, readonly=true`）。Spawn、检视、裁定、汇报、退出。默认用便宜模型控制成本。编排器可以否决它的裁定。

非阻塞体验就来自这种拆分。编排器握着对话，任务握着工作，channel 把它们连起来。你派发一个慢活儿时，编排器把它交接出去，然后继续对你的下一条消息保持响应。

## 进程模型

opca 是单进程。编排器和每个任务都是用 `tokio::spawn` 启的 tokio task，通过 mpsc channel 通信。

这个决定归结到三件事。任务需要共享状态（工作区管理器、冷存储、任务注册表），而进程内共享在这种场景下比 IPC 划算。单进程内 channel 延迟可忽略。任务崩溃则通过 await `JoinHandle`、捕获 panic、把任务翻到崩溃状态，再通知编排器来处理。进程隔离的代价并不值。

代价是单进程承载了一切。如果二进制本身挂了，所有任务也跟着挂。对一个本地单用户工具来说可以接受，而且部署只是一个二进制。

前台 CLI 也是一个 tokio task。它的输入循环永远在读 stdin。输出侧打印编排器的答复和任务完成通知。后台任务默认静默输出：派发时看到一条心跳，完成时一条通知，中间啥也没有，除非你主动问。

## 编排器

编排器是主脑。它持有活跃记忆、归档、冷存储、任务注册表，以及路由逻辑。

### 路由

每条进来的消息都会被分类。简单问题（"X 是干嘛的？"、"提醒我 Y"）由编排器从自己的上下文里立刻作答。任何像真活儿的（"重构 X"、"给 Y 加测试"）会被派发到一个后台任务。编排器也会处理自然语言的状态查询，从任务注册表里拉数据。

### 任务派发与聚焦契约

编排器派发任务时，会签一份聚焦契约 (Focus Contract)。它列出了任务必须汇报的维度：安全风险、破坏性变更、需要确认的决策、阻碍，等等。这份契约会注入到任务的 system prompt 里，而任务的 `report_highlight` 工具要求带一个匹配聚焦列表的 `tag`。

契约是动态的。编排器可以通过 steering channel 发 `update_focus`，在任务运行期间增减维度。活跃维度有一个 8 个的硬上限。要加第九个，编排器必须先移除一个。这样编排器的上下文才不会被亮点噪声淹没。

### 心跳汇聚与 deep_dive

编排器通过三层观察每个任务，默认只盯其中两层。

第 1 层是心跳。每个任务在每个回合结束和每次状态迁移时自动推一条。大概 50 tokens：状态、进度、任务此刻在干什么。编排器把它们汇聚进任务注册表。

第 2 层是亮点。任务调用 `report_highlight` 时推一条，打上聚焦维度的 tag。每条 100 到 300 tokens。它们进入编排器的活跃上下文，所以编排器回复用户时能自然地提到它们。

第 3 层是完整消息历史。它从不被推送。编排器在需要详细理解某个任务的推理时，通过 `deep_dive` 工具按需拉取片段。

这种分层让编排器能同时管很多任务而不撑爆上下文窗口。它通过第 1、2 层看见每个任务的轮廓，只在某件事确实需要时才为第 3 层的细节买单。

### 召回

编排器自己决定要不要召回。用户说完话时，一次后台关键词搜索会跑向归档。如果命中，相关摘要会落入上下文，且不增加首响延迟。当一个问题看起来在引用过去的工作时，编排器也可以显式调用 `recall`。

### 冲突预测

派发之前，编排器会对每个任务可能触碰的文件做一次轻量预测。如果两个任务看起来会重叠，编排器会把它们串行而非并行。这在派发时就能挡掉大多数合并冲突。当冲突还是漏过来了，编排器在合并阶段会尝试自动解决，失败则升级给用户。

## 任务

一个任务是一个后台 worker，有自己的 agent loop、记忆、工作区、聚焦契约和工具注册表。可以多个并行。

### agent 运行循环

循环是标准的回合制 agent 周期：构建上下文、调用 provider、执行任何工具调用，重复直到任务声明完工或需要输入。每个回合以推送一条心跳结束。工具调用按效果分类（read、write、append、process），框架决定哪些可以并行。read 类工具一起跑，write 类工具串行。

### steering 与 follow-up 队列

任务通过两个队列接收外部输入。

**steering 队列**装的是任务正在干活时到来的消息。编排器的 `update_focus` 就落在这里，任务在回合之间取走。这就是聚焦契约在飞行中被调整的机制。

**follow-up 队列**装的是任务空闲、等待或完工时到来的消息。一个应该重启某个 `Waiting` 任务的用户跟进就放在这里。

这一对队列是非阻塞体验背后的机制。任务从不同步阻塞去向编排器请求，编排器也从不同步等待任务。一切走 channel。

### 三层上下文

每个任务维护着前文在编排器下面描述过的三层上下文。心跳和亮点向上流。完整历史留在任务本地，只通过 `deep_dive` 拉取。

### 生命周期状态机

每个任务都经历同一组带名字的状态。每次迁移自动推一条心跳。

```
💤 Sleeping → 🌅 Waking → 🤔 Pondering → 🔨 OnIt
   ↓ on completion          ↓ on input needed
✅ Delivered                🫥 Waiting
   ↓ risk assessment             ↓ on reply / ↓ on timeout
🔍 Reviewing → 📦 Archived       OnIt / ✂️ Axed
😵 Stuck → ✂️ Axed → 📦 Archived
```

状态及其含义：

| 状态          | 含义                                                      |
|---------------|-----------------------------------------------------------|
| 💤 Sleeping   | 刚 spawn。加载 config、插件、上下文。                     |
| 🌅 Waking     | 初始化工作区，加载上下文。                                |
| 🤔 Pondering  | 思考方案（一个规划回合）。                                |
| 🔨 OnIt       | 执行中。干实际的活儿。                                    |
| 🫥 Waiting    | 需要用户或编排器的输入。                                  |
| 🔍 Reviewing  | 完工自检，或正在被审计。                                  |
| ✅ Delivered  | 完成。成果等待接受。                                      |
| 😵 Stuck      | 卡住。需要帮助才能继续。                                  |
| ✂️ Axed       | 被用户或编排器取消。                                      |
| 📦 Archived   | 完全归档。工作区已安排清理。                              |

合法迁移：

- `Sleeping` 到 `Waking`，初始化时。
- `Waking` 到 `Pondering`，第一回合时。
- `Pondering` 到 `OnIt`，执行开始时。
- `OnIt` 到 `Pondering`（下一回合）、到 `Waiting`（需要输入时）、到 `Delivered`（完工时），或到 `Stuck`（卡住时）。
- `Waiting` 到 `OnIt`（回复时），或到 `Axed`（超时时）。
- `Delivered` 到 `Reviewing`（审计介入时），或直接到 `Archived`（低风险跳过审计时）。
- `Reviewing` 到 `Archived`（接受时）、回到 `OnIt`（拒绝时），或 `Axed`（丢弃时）。
- `Stuck` 到 `OnIt`（援兵到达时），或 `Axed`（放弃时）。
- 任意状态到 `Axed`，取消时。
- `Axed` 到 `Archived`，清理之后。

这些拟人化的名字是刻意的。它们让任务列表一眼可扫，而且干净地对应任务实际在做的事。你从 `/tasks` 就能看出某个任务还在想、正在干，还是卡在那等你。

## 审计

审计 agent 是任务的一个只读特化版本，不是独立的顶层概念。它复用任务的抽象、记忆、聚焦契约和亮点机制。区别只在配置：`role=Auditor`、`readonly=true`，外加一个极简生命周期。

### 极简生命周期

审计不走 Sleeping、Waking、Pondering 那一套。它的生命周期刻意做得很短：

```
Spawn → Inspect → Judge → Report → Die
```

### 审计模式

默认情况下，审计是黑盒的。它读 diff、读最终摘要、跑测试。它不看任务的推理链。如果 diff 里有可疑之处，审计可以用 `deep_dive_task_context` 拉取任务完整历史的片段，检查导致某处变更的推理。审计自己决定什么时候深挖。

### 报告结构

裁定以结构化 JSON 返回：

```json
{
  "verdict": "pass",
  "confidence": 0.86,
  "findings": [
    {
      "severity": "minor",
      "location": "auth.rs:42",
      "issue": "token refresh misses the clock-skew edge case"
    }
  ],
  "summary": "Solid overall. One boundary condition to revisit in token refresh."
}
```

`verdict` 是 `pass`、`warn`、`fail` 之一。`confidence` 是 0.0 到 1.0 的分数。每条 finding 带严重度和位置。

### 权力链与否决

权力链是：审计（顾问，检视并裁定）到编排器（决策者，可否决裁定）到用户（高风险工作的最终决定权）。编排器是决策者，审计是顾问。如果审计返回了编排器不同意的裁定，编排器可以否决。高风险任务总是升级给用户做最终决定。

### 成本控制

审计默认用便宜模型。理由是便宜模型足够看出"测试挂了"或"这份 diff 动了 auth 却没测试"，而每个完工任务都触发审计时，成本涨得很快。对真正高风险的工作，编排器会升级到更强的 model。你可以通过 config 里的 `model.audit_model` 按项目覆盖审计 model。

## 记忆

记忆是分形的。同一种 `Memory<T>` 结构在三层复用。

```
Memory<T> {
  active: Vec<T>          // current context window
  archive: Store          // compacted history in SQLite
  index: MultiIndex       // keyword, time, task_id, tag

  compact()               // when the window nears its cap,
                           //   compress old items into the archive
                           //   and index them

  recall(query) -> Vec<T> // search the archive by index
  remember(item, tags)    // write to the archive and index it
}
```

### 三个区域

- **活跃 (Active)。** 当前上下文窗口。agent 此刻能看到的东西。
- **归档 (Archive)。** SQLite 里的压缩历史，沿多个维度建了索引，检索时不必重读全部。
- **冷存储 (Cold Store)。** 跨会话的持久记忆。归档到这里的任何东西跨会话存活，之后随时可召回。

### 分形复用

同一个组件在每一层都出现。

- **任务**用 `Memory<Message>`。活跃区是 loop 内的上下文。归档区存着完整的第 3 层历史。
- **编排器**用 `Memory<ConversationEvent>`。活跃区是主上下文。归档区存着压缩历史。
- **冷存储**用 `Memory<SessionSummary>`。跨会话持久。

因为形状一致，压缩和召回逻辑只写一次，处处复用。

### 索引维度

归档沿五个维度建索引，所以召回可以从多个方向切。

| 维度      | 索引的内容                                       |
|-----------|--------------------------------------------------|
| keyword   | 对分词后的内容建倒排索引。                       |
| time      | 时间范围查询。                                   |
| task_id   | 某个任务产出的所有东西。                         |
| tag       | 从亮点带过来的聚焦 tag。                         |
| semantic  | 嵌入向量检索（可插拔的 model）。                 |

### 压缩

压缩把旧条目从活跃窗口挪进归档。编排器应用特定规则。

完工任务的亮点会被压成一条最终摘要。进行中任务的旧亮点随新亮点到来而滚动丢弃。心跳对每个任务只保留最新一条。活跃窗口装着最近条目加摘要。

活跃上限可通过 `memory.max_active_tokens` 配置。

## 工作区

每个任务在隔离的工作区里干活，并发任务互不踩脚。`WorkspaceManager` 自动挑合适的策略。

### 三种策略

**GitWorkspace** 是 git 项目的默认。它跑 `git worktree add` 创建一个链接工作树。这是最快也最干净的选项，且能为合并阶段产出真正的 git diff。

**MirrorWorkspace** 处理非 git 项目。它在 `.agent/mirror/` 下创建一个全新 git 仓库，导入项目的文本文件，再为每个任务 check out 一个 worktree。非 git 项目的合并流程是：抽出 worktree 的 diff，作为 patch 应用到原始项目目录，再刷新 mirror 基线。

**CopyWorkspace** 是兜底。它做一次完整的递归目录拷贝。磁盘占用最高、创建最慢，但哪儿都能跑，不依赖 git。

### Copy-on-Write

文件系统支持时，mirror 创建会走 Copy-on-Write。macOS APFS 上用 `clonefile`，Linux btrfs 和 xfs 上用 `reflink`。其他文件系统退化为完整递归拷贝。CoW 让 mirror 创建很便宜，即便在大项目上，因为数据块在写入前是共享的。

### agentignore

`.agentignore` 文件（语法同 `.gitignore`）把重型目录排除在 mirror 导入和拷贝遍历之外。常见目标是 `target/`、`node_modules/`、`dist/`、`.venv/`。被排除但 agent 仍想读的目录，会在工作区创建后从源项目符号链接过来，这样读取照常工作，又不用付拷贝代价。

二进制文件永不进入内部 mirror。git diff 对二进制没意义，还会让 mirror 无谓膨胀。

### 清理

合并后的工作区会保留一段可配置的延迟（默认 3 天）再删除。这给你一个窗口，在任务产物消失前先检视。冷存储不受工作区清理影响。归档进去的东西永远可召回。

## 完工流水线

任务声明完工时，进入一个五阶段完工流水线。

```
① Freeze
   worktree frozen (read-only)
   final summary generated
   heartbeat: ✅ Delivered
   Orchestrator + CLI notified

② Review
   risk assessment runs
   low risk -> rule checks only (compile, tests)
   medium/high risk -> Audit agent dispatched
   Orchestrator decides: auto-merge, forward to user

③ Merge
   conflict detection (predicted at dispatch time)
   no conflict -> merge directly
   conflict -> Orchestrator attempts auto-resolve
   auto-resolve fails -> ask the user

④ Memorialize
   final summary -> Orchestrator active memory
   full context + diff -> cold store (indexed)
   Orchestrator runs compaction

⑤ Cleanup
   worktree scheduled for delayed removal (default 3 days)
   cold store entries retained (memory is never lost)
```

### 风险分级

风险决定审计要不要跑。低风险 diff（20 行以内、纯文档）跳过审计 agent，改跑规则检查：能否编译、测试过不过。中高风险工作走完整审计。阈值可通过 `audit.risk_threshold` 配置。

### 依赖链

任务可以声明对其他任务的依赖。任务 A 完工合并后，任何依赖 A 的任务 B 会自动激活。B 基于主分支合并后的状态拿到一个全新的工作区，所以它能看到 A 的改动，无需手动交接。

## 扩展系统

opca 自带三个独立的扩展点。它们被刻意分开，而不是塞进一个 plugin API。

| 类型         | 文件                       | 作用                                            |
|--------------|----------------------------|-------------------------------------------------|
| Context      | `AGENTS.md`, `skills/*.md` | 注入 system prompt 的 Markdown。                |
| Capability   | `mcp.json`                 | 以子进程方式 spawn MCP server。                 |
| Hook         | `hooks.toml`               | 生命周期拦截，有些能阻断。                      |

### Context

Context 扩展是纯 Markdown。顶层的 `AGENTS.md` 教 agent 在这个项目里怎么想、怎么做。`skills/*.md` 下的 skill 文件带可选的 YAML frontmatter，里面有用于相关性匹配的元数据，正文会注入任务的 system prompt。skill 按关键词重叠对任务描述打分。一个和任务描述零关键词重叠的 skill 不会加载，以此保持 prompt 小巧。

`AGENTS.md` 支持 `@import` 行，把相对于导入文件的其他文件内联进来。导入是深度优先的，每个文件最多内联一次，从而打断环。

### Capability

Capability 扩展 spawn 一个或多个 MCP (Model Context Protocol) server 作为子进程。每个 server 通过 stdin/stdout 说 JSON-RPC 2.0。client 驱动三个方法：spawn 时 `initialize`、`tools/list` 枚举工具、`tools/call` 调用一个。

来自 MCP server 的工具以 `mcp__<server>__<tool>` 命名，避免和内置工具及其他 server 的工具冲突。每个 server 是独立子进程，所以崩溃隔离在该 server。主 agent 进程不受影响。

### Hook

Hook 拦截生命周期。覆盖四个事件层级。

| 层级          | 事件                                                                                        | 能阻断         |
|---------------|---------------------------------------------------------------------------------------------|----------------|
| Session       | `on_session_start`, `on_session_end`                                                        | 否             |
| Orchestrator  | `on_user_message`, `on_pre_dispatch`, `on_post_dispatch`, `on_task_highlight`, `on_recall`, `on_merge_pre`, `on_merge_post` | 仅 `merge_pre` |
| Task          | `on_task_create`, `on_task_freeze`, `on_task_reject`, `on_task_archive`, `on_pre_tool_use`, `on_post_tool_use` | 仅 `pre_tool_use` |
| Audit         | `on_audit_start`, `on_audit_report`, `on_audit_override`                                    | 否             |

只有 `on_pre_tool_use` 和 `on_merge_pre` 遵守 `Deny` 结果。其他每个事件上，deny 只会被记日志，操作照常进行。能阻断的 hook 是让规则从建议升级为强制的关键。

识别五种 handler 类型。`command` spawn 一个子进程，把 payload 作为 JSON 写到它的 stdin，再解析 stdout 决定结果。`http` 把 payload 作为 JSON POST 出去，按同样的形状规则解析响应。`mcp_tool`、`prompt`、`agent` 是占位符，目前只记日志并返回 `Continue`，等下游依赖接通。

payload 含一个 `data` 字段，带着事件特定的上下文。hook config 上可选的 `matcher` 子串过滤器，能让热路径不必在每个事件上都 spawn 子进程。

### 插件打包

插件只是一个文件夹，把三种扩展的任意组合打包到一个 `plugin.toml` manifest 后面。插件不引入任何新机制，它为分发而存在。如果你只需要一种扩展，可以跳过插件，直接把相关文件丢进项目即可。

插件声明 keywords，让它的工具只对相关任务激活。没有 keywords 的插件永远开启。否则，只要它的任一 keyword 作为子串出现在小写化的任务描述里，插件就激活。编排器对每个任务最终启用哪些插件的工具有最终决定权。

完整的插件编写指南，包括 Docker helper 的实战 walkthrough，见 [plugins.md](plugins.zh-CN.md)。

## Provider

`Provider` trait 是每个 LLM 后端背后的抽象。它也是让系统其余部分可测试的关键支点。

```rust
trait Provider {
    async fn stream(&self, messages: &[Message], tools: &[ToolDef])
        -> Stream<Event>;
}
```

生产实现调用真实 API。`AnthropicProvider` 驱动带 SSE 流的 Messages API。`OpenAIProvider` 对接 Chat Completions。`GeminiProvider` 对接 Google Gemini。三者都通过同一种 `ProviderEvent` 形状流事件，所以 agent loop 不关心自己在和哪个后端对话。

`ScriptedProvider` 是测试替身。你编程一组响应序列（先一个工具调用，再一段文本，再完工），编排测试就断言系统对某种 LLM 行为作何反应。这就是把 LLM 的不确定性挡在单测之外的东西。测试回答的是"给定 LLM 说了 X，系统会怎么做？"，而不是"LLM 会说什么？"。

### 零拷贝上下文

构建 LLM 上下文时用 `Cow<'_, [Message]>` 引用，而不是克隆整个消息向量。在大会话上，这避免了每个回合一次可观的分配和拷贝。context builder 是借来的，不是拥有的。

## 设计取舍

上面的架构反映了十二个深思熟虑的决定，这里提炼成一张总表。每个都是在真实的备选方案里挑出来的。

| 决定                                            | 选择                                              | 被否决的备选                        | 理由                                                                            |
|-------------------------------------------------|---------------------------------------------------|-------------------------------------|---------------------------------------------------------------------------------|
| 异步运行时                                      | tokio                                             | asupersync, async-std               | 最成熟、文档最好、长期维护有保障。                                              |
| 进程模型                                        | 单进程，tokio task，channel                       | 多进程，IPC                         | 进程内共享状态便宜。崩溃隔离靠 JoinHandle panic 捕获。                          |
| 非阻塞 CLI                                      | 后台静默，完成通知，手动查询                      | 双面板 TUI                          | 最容易交付。覆盖了大部分体验。以后可升级。                                      |
| 生命周期命名                                    | 拟人化状态（Sleeping, OnIt, ...）                 | 临床化状态（Created, Running）      | 任务列表一眼可扫。对应实际工作。                                                |
| 上下文分层                                      | 三层，编排器看 1 和 2                             | 单一扁平上下文                      | 编排器管很多任务而上下文不膨胀。                                                |
| 记忆                                            | 三层分形 `Memory<T>`                              | 每层独立存储                        | 压缩和召回写一次，处处复用。                                                    |
| 工作区隔离                                      | git worktree 加内部 mirror 加拷贝                 | 单一策略                            | git 项目用原生 worktree；非 git 也有隔离。                                      |
| 审计                                            | 只读的任务特化版本                                | 独立的顶层概念                      | 复用任务抽象、记忆、聚焦、亮点。只是配置不同。                                  |
| 完工流水线                                      | 五阶段（Freeze, Review, Merge, Memorialize, Clean）| 临时处理                            | 可预测，各阶段独立可测。                                                        |
| 扩展系统                                        | 三个独立点，插件作为打包                          | 统一的 plugin API                   | 业界没人用统一 API。插件是 bundle，不是机制。                                   |
| 测试                                            | TDD，三层，Provider trait 作支点                  | 仅集成测试                          | LLM 不确定性隔离在 trait 后面。单测保持确定性。                                 |
| 借鉴 pi_agent_rust                              | ToolEffects、steering 和 follow-up、零拷贝        | fork 该项目                         | fork 会绑死在单一维护者的 runtime 上。借想法，不借代码。                        |

## 开放问题

有几件事还没定，可能调整。如果你的工作碰到它们，请浮现出来。

1. **冷存储的语义嵌入 model。** 本地（candle 加 all-MiniLM）还是 API（OpenAI embeddings）。影响离线可用性。
2. **工作区清理延迟默认值。** 目前 3 天。需要真实用户反馈来调。
3. **CLI 行编辑器。** 现在用 `reedline`。要不要换 `rustyline` 或自己手搓以获得更细控制，待定。
4. **会话持久化形态。** 现在是 JSONL 主加 SQLite 索引。要不要把索引折进 JSONL reader，待定。
5. **MCP SDK。** `rmcp` 是 Rust MCP SDK。client 现在是在 `extensions/mcp.rs` 里手搓的；要不要迁移，待定。
