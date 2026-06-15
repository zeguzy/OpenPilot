## Context

opca 的 Task 生命周期是严格单向的状态机（`lifecycle/status.rs`）。关键
约束：`Delivered → Reviewing | Archived` 是终态出口，**没有任何从终态
回到活跃态的转换**。`Archived` 是唯一 terminal 状态。

当前 Task 完成后的处理路径存在三个断裂点（经代码调研确认）：

1. **生产 CLI 未接线 CompletionPipeline**。`real.rs` 的 `poll_loop` 检测到
   `TaskStatus::Delivered` 时只发 `Notification::Completed` 给 TUI，不调用
   `CompletionPipeline::run`。5 阶段 pipeline（Freeze→Review→Merge→
   Memorialize→Cleanup）仅存在于测试中。
2. **AuditAgent 生产从未 spawn**。`pipeline.rs` 的 `review_stage` 对 High
   risk 直接返回硬编码 `AuditVerdict::Warn`，不调 `AuditAgent::new`。
3. **DependencyGraph successor 激活是 stub**。`drain_successors` 返回值
   后只有 `tracing::info!("...successor(s) activated")`，无真实 dispatch。

这三个断裂意味着：continuation loop 不是在现有"正常工作的 pipeline"上加
一层，而是**与 pipeline 的生产接线、audit 的真实化、successor 的落地
共同构成一个完整的工作链**。本 design 将这些视为同一提案的前置 task。

业界调研提供了成熟的设计参照。最相关的两个：

- **ralph-loop / ulw-loop**（oh-my-openagent）：self-referential loop until
  `<promise>DONE</promise>`。两者的本质区别不在迭代数（100 vs 500），
  而在**完成判定的信任模型**——ralph 信任 agent 自报，ulw 强制独立 Oracle
  验证。ulw 的双层判定（DoneClaim → AdversarialVerify → confirmed）与 opca
  的 Task（执行）+ Audit（验证）架构天然同构。
- **Claude Code 的 guardrail 原则**：步数/预算上限是**财务断路器**，不是
  完成判定。主终止必须是结构化的。代理信号（意图、进度、努力、记忆、
  合理答案）不能当完成证据。终止原因应分类，不同原因走不同决策路径。

## Goals / Non-Goals

**Goals:**

- G1：Task 完成后，系统能基于 Audit verdict、测试结果、Task 自声明，在
  预算上限内**自动派发新 Task 续跑**，无需用户手动干预。
- G2：续跑判定遵循**双层完成协议**——Task 自报 done 不终止链；必须 Audit
  验证 `confirmed` 才终止。
- G3：续跑链有**多层安全阀**——最大迭代、最大成本、最大时长、无进展检测、
  用户随时可 `/stop-continuation`。
- G4：续跑判定逻辑是**可插拔策略**（`ContinuationPolicy` trait），默认实现
  可被插件替换，不硬编码到 pipeline。
- G5：每个续跑轮次是一个**全新 Task**（新 task_id、新 workspace），不破坏
  现有状态机的单向不变量。跨轮状态通过 Cold Store + 续跑 prompt 传递。
- G6：CompletionPipeline 在生产 CLI 路径上被真正接线，AuditAgent 被真实
  spawn，successor dispatch 真正落地——这三个前置缺口在本提案内填补。

**Non-Goals:**

- N1：不实现"Task 自循环"（同一 Task 从 Delivered 回到 OnIt）。这违反状态
  机不变量，且 workspace 已 freeze、所有权已转移。
- N2：不实现跨 session 的长时编排（类似 start-work 的 Prometheus plan +
  boulder.json checkbox 推进）。这是更高层的编排能力，超出本提案范围。
- N3：不实现 RL reward shaping 或 trajectory-level 的策略学习。
- N4：不实现远程/分布式的 managed session 协调（类似 Devin 的 coordinator
  多 VM 调度）。opca 保持单进程。
- N5：不修改现有的 `Memory<T>`、`Workspace`、`Provider` 抽象。续跑复用
  现有基础设施。
- N6：不实现续跑链的持久化/崩溃恢复（进程重启后续跑链丢失）。这是后续
  工作，当前续跑链只存活于进程生命周期内。

## Decisions

### D1: 续跑 = 新 Task，不复活原 Task

**决策**：续跑通过 `dispatch_task` 派发一个**新 Task**（新 task_id、新
workspace、新 provider），携带 `parent_task_id` 关联到前一轮。

**理由**：
- `status.rs` 的 `is_valid_transition` 明确禁止 `Delivered → OnIt`。
  新增这条转换会破坏状态机不变量，影响所有依赖该不变量的代码（heartbeat、
  lifecycle tracker、completion pipeline 的 Freeze 阶段假设 workspace 可
  freeze）。
- 原 Task 的 workspace 在 Freeze 阶段已变只读，provider/workspace所有权
  已转移给 pipeline。复活原 Task 需要逆向这些操作，复杂且脆弱。
- 新 Task 走完整 `Sleeping → ... → Delivered` 生命周期，天然复用现有的
  dispatch、heartbeat、focus contract、audit 全套基础设施。

**替代方案（已否决）**：
- *Task 自循环*：新增 `Delivered → OnIt` 转换。否决理由见上。
- *原 Task 挂起续跑*：Task 不走 Delivered，而是进入新状态 `Continuing`。
  否决：状态机膨胀，且 Freeze/Memorialize 语义混乱（还没 freeze 就续跑？）。

**关联**：新 Task 的 workspace 基于**已合并的 main 分支**创建（而非前一轮
的 workspace），确保续跑基于最新代码。前一轮的 diff、audit report、失败
反馈通过续跑 prompt 注入。

### D2: ContinuationCoordinator 作为 Pipeline 的可插拔组件

**决策**：新增 `ContinuationCoordinator`，作为 `CompletionPipeline` 持有的
策略组件，定位类比 `DependencyGraph`——纯策略、可单测、被 pipeline 持有。

```
CompletionPipeline
  ├─ freeze_stage
  ├─ review_stage        → 产出 AuditReport（含 DoneClaim 验证）
  ├─ merge_stage
  ├─ memorialize_stage   → 续跑链上下文累积到 Cold Store
  ├─ cleanup_stage
  └─ continuation_stage  → ContinuationCoordinator.decide() → Option<ContinueDecision>
```

**理由**：
- Pipeline 已是 Task Delivered 后的唯一处理路径，持有
  `Arc<Mutex<Orchestrator>>` 可直接调 `dispatch_task`。在 pipeline 末尾加
  一个 `continuation_stage` 是改动最小的注入点。
- `DependencyGraph`（`drain_successors`）是现成模板——同样是"pipeline 持有、
  纯策略、返回值驱动后续动作"的组件形态。
- 把策略（是否续跑）与流程（如何完成）分离，策略可独立演进、可被插件替换。

**替代方案（已否决）**：
- *独立 ContinuationCoordinator 监听事件*：否决——Orchestrator 目前没有
  "Task 完成"的事件源（join_handle 存了但从不 await），要先建事件总线，
  改动面过大。
- *Orchestrator 内联续跑逻辑*：否决——Orchestrator 职责已重（路由、dispatch、
  heartbeat 聚合、recall），再加续跑策略会导致上帝对象。

### D3: Continue 作为 CompletionOutcome 的新变体

**决策**：`CompletionOutcome` 新增 `Continue { reason, next_prompt_seed,
chain_id, iteration }` 变体。

```rust
pub enum CompletionOutcome {
    Merged,
    PendingReview,
    Rejected(String),
    Failed(String),
    Continue {                          // 新增
        reason: ContinuationReason,
        next_prompt_seed: String,
        chain_id: ContinuationChainId,
        iteration: u32,
    },
}

pub enum ContinuationReason {
    AuditRejected { verdict: AuditVerdict, findings: Vec<Finding> },
    TestsFailed { failures: Vec<String> },
    TaskSelfReportedIncomplete { remaining: Vec<String> },
    SuccessorActivated { successor_count: usize },
}
```

**理由**：`CompletionOutcome` 已是 pipeline 的决策出口协议。新增变体是
表达"需要续跑"的最自然方式，且 `match` 该枚举的地方会被编译器强制处理
新分支（Rust 的穷尽匹配），不会遗漏。

**替代方案（已否决）**：*复用 `Failed` + 附带 reason 字段*——否决：`Failed`
语义是"pipeline 自身错误"，与"任务需要续跑"语义不同，混用会导致下游决策
歧义。

### D4: 双层完成判定（借鉴 ULW-Loop）

**决策**：continuation 链的终止不只看 Task 自报 done，必须经 Audit agent
验证。Audit 的 `AuditVerdict` 细化为四值：

```rust
pub enum AuditVerdict {
    Confirmed,              // 唯一 pass：验证通过，continuation 链可终止
    FalsePositive,          // Task 声称完成但验证发现未实际完成
    NeedsFix,               // 大体完成但有可修复的缺陷
    NeedsHumanReview,       // 无法自动判定，升级到用户
}
```

只有 `Confirmed` 终止 continuation 链。`FalsePositive` / `NeedsFix` 触发
带反馈的续跑（audit findings 注入下一轮 prompt）。`NeedsHumanReview` 停止
续跑并通知用户。

**理由**：
- 调研结论（代理信号拒绝）：agent 自报 done 不可信。意图、进度、努力、
  合理答案都不能当完成证据。唯一合法证据是**可复现的 fresh artifact**。
- ULW-Loop 的 Sisyphus 完成契约验证了这套模型：DoneClaim → AdversarialVerify
  → confirmed 是唯一 pass，失败重派。opca 的 Task/Audit 分离天然适配。
- 现有 `AuditVerdict`（Pass/Warn/Fail）语义不够精确——`Warn` 既可能是"基本
  ok 有小问题"（应 NeedsFix 续跑），也可能是"可疑需深查"（应
  NeedsHumanReview）。四值拆分消除歧义。

**替代方案（已否决）**：*保留三值 Pass/Warn/Fail，在 Coordinator 里映射*——
否决：映射逻辑分散在多处，且 Warn 的歧义无法在映射层解决。源头拆分更清晰。

**与现有 Audit spec 的关系**：现有 spec 定义 `verdict (pass/warn/fail)`。
本提案 MODIFIED 该 requirement 为四值。`pass → Confirmed`，`fail → NeedsFix
或 NeedsHumanReview`（由 confidence 区分），`warn → FalsePositive 或
NeedsFix`。现有 `Orchestrator 可 override` 的 requirement 保留——override
可将任意 verdict 改为 `Confirmed` 以强制终止链。

### D5: ContinuationBudget——多层安全阀

**决策**：每条 continuation 链绑定一个 `ContinuationBudget`，任一维度耗尽
即停止续跑：

```rust
pub struct ContinuationBudget {
    max_iterations: u32,          // 默认 10；ultrawork 模式 50
    max_total_cost_usd: f64,      // 默认 5.0
    max_total_duration: Duration, // 默认 30 分钟
    max_no_progress_rounds: u32,  // 默认 2（连续 N 轮无实质进展）
    current_iteration: u32,
    accumulated_cost_usd: f64,
    started_at: Instant,
    consecutive_no_progress: u32,
}
```

**理由**：
- **max_iterations**：调研显示 ralph-loop 默认 100，但那是一个 agent 内的
  session 迭代；opca 的"一轮"是一个完整 Task（含独立 workspace、provider、
  audit），成本远高于 session 迭代。默认 10 是保守的工程默认值，可通过
  config 和 `/continue --max-iterations N` 覆盖。
- **max_total_cost_usd**：财务断路器。Claude Code 用 `max_budget_usd` 作
  guardrail，Devin 用 ACU limit。opca 每轮 Task 的成本由 provider 层计量，
  Coordinator 累加。
- **max_total_duration**：防止 Task 内部卡死（如 provider 超时重试无限循环）
  导致续跑链永不结束。
- **max_no_progress_rounds**：无进展检测（借鉴 ralph-loop 的
  `no-progress-turn-detector` 和 opencode 的 doom loop detector）。连续 N
  轮的 diff 为空或与前一轮实质相同，判定无进展。`NoProgressDetector` 基于
  diff 行数变化 + 文件集合差异的启发式。

**耗尽时的行为**：停止续跑，将链标记为 `BudgetExhausted`，通知用户当前进度
（已完成几轮、累计成本、最后一个 audit verdict）。**不**自动回滚已合并的
工作——已 merge 的轮次保留，用户决定后续。

**替代方案（已否决）**：*只设 max_iterations*——否决：单维度不够。成本失控
（每轮调用昂贵模型）、时长失控（provider 挂起）、无进展死循环都无法靠迭代
数单独捕获。

### D6: 每轮新 Workspace，状态通过 Cold Store + Prompt 传递（reset 策略）

**决策**：续跑采用 ralph-loop 的 **reset 策略**——每轮派发新 Task 时，基于
**已合并的 main 分支**创建新 workspace（而非继承前一轮的 workspace）。
跨轮状态通过两条通道传递：

1. **Cold Store**：前一轮的 full context、diff、audit report 存入 Cold Store，
   下一轮的 Task 可通过 `recall` 工具按 `chain_id` 检索。
2. **续跑 prompt**：Coordinator 构造的 `next_prompt_seed` 包含结构化的失败
   反馈——前一轮的 audit findings、失败的测试、未完成的子项。

**理由**：
- 调研显示 ralph-loop 的 reset 策略让 agent 始终在 context window 的"聪明区"
  工作，避免 context 膨胀导致能力退化。opca 的 Task 本身有 `compact()` 机制，
  但续跑链跨多轮时，reset 比单 Task 内 compact 更彻底。
- 新 workspace 基于 main 分支确保续跑在最新代码上工作（前一轮可能已 merge
  部分改动），避免基于陈旧 workspace 续跑引入冲突。
- Cold Store 是 opca 已有的跨 session 持久化机制，天然适合跨轮上下文传递。

**替代方案（已否决）**：*continue 策略（继承前一轮 workspace）*——否决：
前一轮 workspace 已在 Freeze 阶段变只读、在 Cleanup 阶段计划删除。继承需
要逆向这些操作。且 workspace 可能已被其他并行 Task 的 merge 污染。新
workspace 更安全。

**Trade-off**：reset 策略的代价是每轮 workspace 创建开销（git worktree 或
mirror）。CoW（`clonefile`/`reflink`）已将此开销降到很低。

### D7: 终止原因分类学

**决策**：continuation 链的终止原因作为一等公民，走不同的通知和清理路径：

```rust
pub enum ChainTerminationReason {
    ConfirmedComplete,       // Audit Confirmed，链成功完成
    BudgetExhausted(BudgetDimension), // 预算耗尽（哪个维度）
    NoProgress,              // 连续无进展
    UserCancelled,           // /stop-continuation
    NeedsHumanReview,        // Audit 升级，无法自动判定
    TaskError(String),       // Task 不可恢复错误
}
```

**理由**：调研显示 SWE-Master 的 RL reward shaping 把终止原因分类为
DONE / TIMEOUT / MAX_STEPS / MAX_TOKENS / CONTAINER_FAILED，各走不同 reward
路径。opca 虽不做 RL，但终止原因分类对**用户通知**和**后续策略**同样关键：
- `ConfirmedComplete` → 静默通知"链完成"
- `BudgetExhausted` → 突出通知"预算耗尽，已完成 N 轮，累计成本 $X"
- `NoProgress` → 通知"连续 N 轮无进展，可能需要人工介入"
- `NeedsHumanReview` → 排队 pending review

**替代方案（已否决）**：*统一标"失败"*——否决：丢失诊断信息，用户无法判断
是加预算重试、还是修改任务、还是人工接手。

### D8: 生产接线策略——poll_loop 触发 Pipeline

**决策**：修改 `real.rs` 的 `poll_loop`，在 `collect_changes` 检测到
`TaskStatus::Delivered` 时，调用 `CompletionPipeline::run(task_id)`。

**理由**：这是三个前置缺口中最关键的。调研确认 `CompletionPipeline` 已实现
完整 5 阶段但从未在生产路径调用。接线点是明确的——`poll_loop` 已在检测
Delivered 状态，只需把"发 Notification"升级为"触发 Pipeline"。

**并发考虑**：Pipeline 持有 `Arc<Mutex<Orchestrator>>`，在 `poll_loop` 的
异步上下文中调用时需注意锁竞争。Pipeline 的各阶段是顺序 await 的，持锁时间
可能较长。缓解：Pipeline 在独立 tokio task 中 spawn，通过 channel 回传
`CompletionOutcome`，`poll_loop` 不阻塞等待。

**替代方案（已否决）**：*Orchestrator 内部监听 join_handle*——否决：需要
重构 Orchestrator 的事件模型（目前 join_handle 存了从不 await），改动面远
大于在 poll_loop 加一个调用。

### D9: ContinuationChain 状态追踪

**决策**：新增 `ContinuationChain` 结构，由 Orchestrator 的
`continuation_chains: HashMap<ChainId, ContinuationChain>` 注册表追踪。

```rust
pub struct ContinuationChain {
    id: ChainId,
    root_task_id: TaskId,          // 链的第一个 Task
    current_task_id: TaskId,       // 当前活跃的 Task
    budget: ContinuationBudget,
    status: ChainStatus,           // Active / Terminated(reason)
    iterations: Vec<IterationRecord>,
}

pub struct IterationRecord {
    task_id: TaskId,
    iteration: u32,
    outcome: CompletionOutcome,
    audit_verdict: Option<AuditVerdict>,
    cost_usd: f64,
    duration: Duration,
    diff_summary: String,
}
```

**理由**：续跑链需要可观测性——用户 `/continue status` 要看到链的进度，
Coordinator 判定要参考历史迭代（如"连续 N 轮 audit 都是 NeedsFix 同一个
问题"应升级为 NeedsHumanReview）。`IterationRecord` 累积的诊断信息也用于
续跑 prompt 构造（避免下一轮重复同样的错误）。

## Risks / Trade-offs

### R1: 成本失控

**风险**：续跑链在 max_iterations 内调用昂贵模型，累计成本可能远超用户
预期。Devin 的 ACU 和 Claude Code 的 `max_budget_usd` 都是为应对此风险。

**缓解**：`max_total_cost_usd` 作为硬性财务断路器（D5）。每轮派发前检查预算，
超限拒绝派发。续跑链的 Task 默认使用与原 Task 相同的 provider/model，但
config 可指定续跑使用更便宜的 model tier（借鉴 Audit agent 的 cheap model
默认）。用户 `/continue --budget 2.0` 可按链设预算。

### R2: 无进展死循环

**风险**：Task 每轮都声称在修复但 diff 实质不变，或反复修复同一个 audit
finding，导致 doom loop。

**缓解**：`NoProgressDetector`（D5）基于 diff 行数变化 + 文件集合差异的
启发式。连续 `max_no_progress_rounds`（默认 2）轮判定无进展即终止。额外
启发式：如果连续 3 轮的 audit findings 指向同一文件同一问题类别，也判定
无进展并升级到 NeedsHumanReview。

### R3: 状态机不变量破坏

**风险**：如果实现错误地将续跑实现为"原 Task 复活"（新增 Delivered→OnIt
转换），会破坏状态机不变量，影响 heartbeat、lifecycle tracker、completion
pipeline 的 Freeze 语义。

**缓解**：D1 明确续跑 = 新 Task。在 `status.rs` 的 proptest 中新增不变量：
"从任何终态（Delivered/Archived）出发，不存在回到活跃态的路径"。continuation
模块的测试验证"续跑派发的 Task 有新的 task_id 和独立的 lifecycle"。

### R4: Audit stub 导致双层判定失效

**风险**：如果 AuditAgent 的生产 spawn 接线不完整（review_stage 仍是 stub），
双层完成判定（D4）无法工作——所有 verdict 都是硬编码 Warn，continuation
链的行为退化。

**缓解**：tasks.md 将"AuditAgent 生产 spawn"列为 continuation 功能的**前置
task**（Phase 0）。continuation 的集成测试依赖真实 Audit verdict，会暴露
stub 未接线的问题。

### R5: poll_loop 锁竞争

**风险**：Pipeline 持有 `Arc<Mutex<Orchestrator>>`，在 poll_loop 中同步调用
可能阻塞心跳轮询，影响其他 Task 的响应性。

**缓解**：D8 决定 Pipeline 在独立 tokio task 中 spawn，通过 channel 回传
outcome。poll_loop 只投递任务不等待。续跑 dispatch 也异步进行。

### R6: 续跑 prompt 注入的安全风险

**风险**：续跑 prompt 包含前一轮的 audit findings、测试失败信息。如果这些
内容来自不可信源（如 Task 读取了外部文件），可能存在 prompt 注入。

**缓解**：续跑 prompt 由 Coordinator 用结构化模板构造（非直接拼接原始文本）。
audit findings 经 sanitize（截断超长字段、转义控制字符）。续跑 prompt 标记为
系统级 context，Task 的 provider 层不做工具执行（避免 finding 里的伪造工具
调用）。

### R7: 并行 Task 与续跑链的 workspace 冲突

**风险**：续跑链的 Task A（第 3 轮）与用户手动派发的 Task B 可能修改同一批
文件。现有 `Orchestrator predicts dispatch conflicts` 机制会序列化它们，但
续跑链的 file 估算需要 Coordinator 提供。

**缓解**：Coordinator 在派发续跑 Task 时，基于链的 `root_task_id` 的
`estimated_files`（继承自原 dispatch）提供 file 估计，复用现有冲突预测。

## Migration Plan

本提案分三个阶段，每阶段可独立合并和验证：

### Phase 0: 前置接线（无 continuation 功能，填补现有缺口）

1. `real.rs` poll_loop 接线 `CompletionPipeline::run`
2. `review_stage` 真实 spawn `AuditAgent`（替换硬编码 Warn stub）
3. `drain_successors` 返回值触发真实 `dispatch_task`（替换 `tracing::info!`）

**验证**：现有 e2e 测试通过；手动测试 Task 完成后 pipeline 各阶段真实执行。

### Phase 1: ContinuationCoordinator 核心能力

4. 新增 `continuation/` 模块：`ContinuationBudget`、`ContinuationChain`、
   `NoProgressDetector`
5. `CompletionOutcome::Continue` 变体 + `ContinuationReason`
6. `AuditVerdict` 细化为四值（Confirmed/FalsePositive/NeedsFix/NeedsHumanReview）
7. `ContinuationCoordinator` + 默认 `ContinuationPolicy` 实现
8. pipeline 新增 `continuation_stage`，集成 Coordinator

**验证**：continuation 模块单元测试；pipeline 集成测试覆盖 Continue 路径。

### Phase 2: CLI 接入与用户体验

9. `dispatch_task` 支持 `parent_task_id` 参数
10. Orchestrator 新增 `continuation_chains` 注册表
11. `/continue`（手动启动链）、`/stop-continuation`（停止链）slash 命令
12. `.agent/config.toml` 新增 `[continuation]` 段
13. e2e 测试："audit fail → 自动续跑 → audit pass → 终止"完整链路

**验证**：CLI 集成测试；config 解析测试；e2e 链路测试。

**回滚策略**：每个 Phase 是独立 commit 序列。Phase 0 和 Phase 1 可独立回滚
（Phase 1 回滚后 continuation 功能消失但 pipeline 仍工作）。Phase 2 回滚
移除 CLI 命令但 core 能力保留。`[continuation]` config 默认 `enabled = false`，
用户需显式开启。

## Open Questions

1. **续跑链的崩溃恢复**：本提案 N6 明确不做持久化。但如果进程重启，活跃的
   continuation 链丢失是否可接受？用户可能期望"重启后链继续"。这涉及
   `ContinuationChain` 的序列化（类似 boulder.json）和 Orchestrator 重启时
   的链恢复。留待后续提案。

2. **续跑 prompt 的 token 预算**：每轮续跑 prompt 注入前一轮的 audit findings
   + 测试失败 + Cold Store recall 结果。多轮后 prompt 可能膨胀。是否需要对
   续跑 prompt 做 compact？还是依赖 Task 内部的 `compact()` 机制？

3. **ultrawork 模式的触发方式**：proposal 提到 ultrawork 模式 max_iterations=50。
   用户如何选择普通模式（10）vs ultrawork（50）？`/continue --ultrawork`？
   还是 config 全局设置？需要用户体验设计。

4. **续跑链与 DependencyGraph 的交互**：续跑链派发的新 Task 可能也有
   successors。Coordinator 触发续跑与 DependencyGraph 触发 successor 激活
   是否可能冲突（同一 Task 既是续跑结果又触发 successor）？需要明确优先级。

5. **多 provider 续跑**：续跑链是否支持中途切换 provider/model？例如前几轮
   用 Sonnet，后续轮降级到 Haiku 控制成本。config 层面是否需要 per-iteration
   model 配置？
