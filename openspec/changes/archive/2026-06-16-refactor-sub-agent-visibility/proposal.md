## Why

opca 的 sub-agent 系统（`--features sub-agents`）有三个结构性缺陷：

1. **子任务是幽灵。** `drain_and_run_subtasks()` 在 parent 的 tokio task 内同步创建并运行 child Task，但 child 从未注册到 Orchestrator 的 Task Registry。child 有完整的状态机（Sleeping→Waking→…→Delivered），推送 heartbeat 到自己的 channel，但**无人读取**。Orchestrator 看不到 child 的存在，`/subtasks` 查不到它，TUI 无法显示其进度。

2. **同步阻塞。** child 通过 `block_in_place(|| Handle::current().block_on(child.run(...)))` 运行。parent 的 tokio task 被完全冻结——不推 heartbeat、不响应 steering、不处理 cancellation。如果 child 跑 50 轮，parent 对外"消失"几分钟。

3. **子任务不参与续跑。** continuation chain 只追踪顶层 Task。child 即使 Evidence Gate 失败也不会自动续跑，只是返回 `Error(msg)` 给 parent。parent 可以手动再 dispatch，但没有预算限制和自动续跑机制。

本提案将 sub-agent 从"幽灵同步调用"重构为"可见异步任务"，让 child 注册到 Orchestrator、异步执行、通过 channel 回传结果、parent 不被阻塞。

## What Changes

- **子任务注册到 Orchestrator。** `dispatch_subtask` 不再在 parent run loop 内创建 child，而是把 `SubTaskRequest` 交给 Orchestrator 的 `dispatch_subtask` 方法。Orchestrator 创建 child Task（有 `parent_task_id`）、注册到 Task Registry、推送 heartbeat 到 Orchestrator 的聚合 channel。child 的状态变化对 Orchestrator 可见，`/subtasks` 能查到。

- **异步执行 + channel 回传。** parent 调用 `dispatch_subtask` 后不阻塞等待，而是收到一个 ticket ID。child 完成后通过 `SubTaskNotificationQueue`（已有类型但未接线）回传结果。parent 的 run loop 在每轮 steering 检查后轮询 notification queue，拿到结果后注入 `active` 消息并继续执行。

- **Parent 进入 Waiting 状态。** 当 parent 有 pending subtask 且本轮无新工具调用时，transition 到 `Waiting`（已有状态），推送 heartbeat "waiting for N subtask(s)"。Orchestrator 看到 parent 在 Waiting，知道它没卡住，是在等 child。child 完成后 parent 被 steering 唤醒回 `OnIt`。

- **子任务 budget 隔离。** 每个 child Task 有自己的 `MAX_TURNS` 和 Evidence Gate。parent 的 continuation chain 不覆盖 child——child 是独立的 Task，有自己的生命周期。

- **BREAKING（内部协议）**：`drain_and_run_subtasks()` 同步执行逻辑移除，替换为异步 notification 轮询。`DispatchSubtaskTool` 的 `execute()` 返回 ticket ID 而非"已派发"消息。`SubTaskNotificationQueue` 从死代码变为活跃通道。

## Capabilities

### New Capabilities

- `sub-agent-async`: 异步子任务派发——child 注册到 Orchestrator、异步执行、通过 notification queue 回传结果、parent 进入 Waiting 状态等待 child 完成。

### Modified Capabilities

- `task-lifecycle`: Task 的 `Waiting` 状态新增"等待子任务"语义——当 parent 有 pending subtask 时 transition 到 Waiting，child 完成后通过 steering 唤醒回 OnIt。
- `orchestrator-core`: Orchestrator 新增 `dispatch_subtask(parent_id, request)` 方法——创建 child Task、注册到 Registry、设置 parent_task_id、把 notification queue 关联到 parent 的 steering channel。
- `task-continuation`: continuation chain 的 `check_continuations` 新增对子任务完成事件的感知——child Delivered 后检查 parent 是否在 Waiting 且有未处理的 notification。

## Impact

**修改模块**：
- `crates/opca-core/src/sub_agent/dispatch.rs` — `DispatchSubtaskTool::execute()` 返回 ticket ID，不再入队后立即"完成"
- `crates/opca-core/src/sub_agent/lifecycle.rs` — 接入 child Task 创建逻辑
- `crates/opca-core/src/task/run.rs` — 移除 `drain_and_run_subtasks()` 同步执行，替换为 notification 轮询 + Waiting 状态管理
- `crates/opca-core/src/task/task.rs` — Task 持有 `SubTaskNotificationQueue`，支持 Waiting-for-subtask 状态
- `crates/opca-core/src/orchestrator/orchestrator.rs` — 新增 `dispatch_subtask()` 方法
- `crates/opca-core/src/orchestrator/registry.rs` — TaskEntry 新增 `notification_queue` 字段关联 parent-child

**新增依赖**：无（复用现有 channel 基础设施）。

**Feature gate**：所有改动在 `#[cfg(feature = "sub-agents")]` 下，不影响默认构建。
