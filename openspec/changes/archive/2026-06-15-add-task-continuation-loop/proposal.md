## Why

opca 的 Task 生命周期是**一次性**的：Task 到达 `Delivered` 后走 5 阶段
Completion Pipeline，最终 `Archived`。状态机中 `Delivered → Reviewing |
Archived` 是单向的——没有任何从终态回到活跃态的路径。

但真实的编码任务极少一轮完成。当 Audit verdict 是 `Fail`/`Warn`、当测试
或 lint 失败、当 Task 自评估发现还有未完成的子项时，现在的唯一出路是
**通知用户**，由用户手动重新派发。这把"多轮迭代"的负担全部甩给了人，
违背了 opca "background-first, never block the user" 的核心承诺。

现有的 `DependencyGraph`（completion/dependency.rs）只处理**预先声明**的
predecessor → successor 链，且 pipeline 里 successor 激活目前只是
`tracing::info!` 的 stub。它无法基于 Task 的**实际产出**（Audit verdict、
测试结果、Task 自声明）动态决定续跑。

本提案引入 **Continuation Coordinator**：一个可插拔的策略组件，挂在
Completion Pipeline 上，在 Task 完成后判定"是否需要再派一轮"，并在预算
上限内自动派发**新 Task**（复用上下文、基于已合并的 main、携带前一轮的
失败反馈）。这填补了 opca 从"单次派发"到"自驱续跑"之间的能力空白。

## What Changes

- **新增 `CompletionOutcome::Continue` 变体**，携带续跑原因、下一轮 prompt
  种子、与前一轮的关联 ID。Pipeline 的 Review/Merge 阶段产出该变体时，
  Coordinator 接管后续派发。
- **新增 `ContinuationCoordinator` 组件**（`opca-core/src/continuation/`），
  作为 Completion Pipeline 的可插拔策略层，定位类比 `DependencyGraph`：
  纯策略、可单测、被 pipeline 持有。负责：续跑判定、预算追踪、新 Task
  派发、失败反馈注入。
- **新增 `ContinuationBudget`**：每条 continuation 链的硬上限——最大迭代
  数（默认 10，ultrawork 模式 50）、最大累计成本（USD）、最大累计时长、
  无进展轮次阈值。任一耗尽即停止续跑并通知用户。
- **新增双层完成判定协议**（借鉴 ULW-Loop）：Task 自报 `DoneClaim` 不再
  是终止信号；必须经 Audit agent 验证为 `confirmed` 才终止 continuation
  链。非 `confirmed`（`false-positive` / `needs-fix` / `needs-human-review`）
  触发带反馈的续跑，或停止并升级到用户。
- **新增无进展检测**（`NoProgressDetector`）：连续 N 轮的 diff 为空或与
  前一轮实质相同，自动终止续跑，避免"doom loop"。
- **接线生产 CLI**：`real.rs` 的 `poll_loop` 在检测到 `TaskStatus::Delivered`
  时，调用 `CompletionPipeline::run`，使 Pipeline 的 5 阶段（含新的
  continuation 判定）在生产路径上真正生效。
- **落地 successor dispatch**：`DependencyGraph::drain_successors` 返回的
  successors 在 pipeline 内真正调用 `dispatch_task`（替换现有的
  `tracing::info!` stub），这是 continuation 的最小可运行原型。
- **新增 `/continue` 与 `/stop-continuation` slash 命令**：用户可手动启动
  续跑链、或随时停止某条链。
- **BREAKING（内部协议）**：`CompletionOutcome` 枚举新增变体，所有 `match`
  该枚举的地方必须处理 `Continue`。这是 crate 内部 API，不影响 CLI 用户。

## Capabilities

### New Capabilities

- `task-continuation`: 任务完成后的自动续跑能力——ContinuationCoordinator
  的判定策略、ContinuationBudget 的预算追踪、双层完成判定协议、无进展检测、
  以及续跑链的生命周期管理。

### Modified Capabilities

- `completion-pipeline`: Review/Merge 阶段产出新的 `Continue` outcome 变体；
  Memorialize 阶段为续跑链累积跨轮上下文；successor dispatch 从 stub 升级
  为真实派发；Pipeline 在生产 CLI 路径上被接线。
- `orchestrator-core`: Orchestrator 在 Task Delivered 后触发 Completion
  Pipeline（含 continuation 判定）；新增 `continuation_chains` 注册表追踪
 活跃的续跑链；dispatch_task 支持携带 `parent_task_id` 建立链式关联。
- `audit-agent`: Audit agent 在生产路径上被真实 spawn（替换 review_stage
  的 stub）；AuditReport 新增 `DoneClaim` 验证字段（verdict 细化为
  confirmed / false-positive / needs-fix / needs-human-review）以驱动
  continuation 决策。

## Impact

**新增模块**：
- `opca-core/src/continuation/mod.rs` — 模块根
- `opca-core/src/continuation/coordinator.rs` — ContinuationCoordinator
- `opca-core/src/continuation/budget.rs` — ContinuationBudget + 耗尽检测
- `opca-core/src/continuation/policy.rs` — 续跑判定策略（可插拔）
- `opca-core/src/continuation/no_progress.rs` — 无进展检测器
- `opca-core/src/continuation/chain.rs` — ContinuationChain 状态追踪

**修改模块**：
- `opca-core/src/completion/pipeline.rs` — `CompletionOutcome::Continue`
  变体、successor dispatch 落地、Coordinator 集成点
- `opca-core/src/completion/dependency.rs` — `drain_successors` 返回值
  供 Coordinator 使用
- `opca-core/src/orchestrator/orchestrator.rs` — Delivered 事件触发
  Pipeline、continuation_chains 注册表、dispatch_task 支持 parent_task_id
- `opca-core/src/audit/agent.rs` — 生产 spawn 接线、DoneClaim 验证
- `opca-core/src/audit/report.rs` — AuditVerdict 细化、DoneClaim 字段
- `opca-cli/src/real.rs` — poll_loop 接线 CompletionPipeline
- `opca-cli/src/commands.rs` — `/continue`、`/stop-continuation` 命令

**新增依赖**：无（纯 Rust 标准库 + tokio，复用现有 channel 基础设施）。

**测试影响**：新增 `continuation/` 模块的单元测试（rstest + proptest）；
扩展 completion pipeline 的集成测试覆盖 `Continue` 路径；新增 e2e 测试
验证"audit fail → 自动续跑 → audit pass → 终止"的完整链路。

**配置影响**：`.agent/config.toml` 新增 `[continuation]` 段——默认最大
迭代、默认预算、无进展阈值、是否默认开启双层验证。

**前置依赖**：本提案假设 `CompletionPipeline` 已在生产 CLI 接线、AuditAgent
已真实 spawn。这两项在 tasks.md 中作为前置 task 处理（若尚未落地）。
