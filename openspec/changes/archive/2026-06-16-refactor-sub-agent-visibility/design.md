## Context

当前 sub-agent 实现在 `drain_and_run_subtasks()`（run.rs）中同步创建 child Task 并通过 `block_in_place` 运行。这导致三个问题：child 不可见于 Orchestrator、parent 被阻塞、child 无续跑能力。

已有但未接线的设施：
- `SubTaskNotificationQueue`（`Arc<Mutex<Vec<SubTaskNotification>>>`）— 定义在 dispatch.rs，仅自身测试引用
- `SubTaskNotification`（Completed/Failed）— 定义在 dispatch.rs，从未在生产路径使用
- `dispatch_task(..., parent_task_id: Option<String>)` — Orchestrator 已支持 parent linkage
- `TaskEntry.parent_task_id` — Registry 已有字段
- `Waiting` 状态 — 状态机已有，目前仅用于 `request_clarification`

## Goals / Non-Goals

**Goals:**
- G1：child Task 注册到 Orchestrator Task Registry，heartbeat 可见，`/subtasks` 可查。
- G2：parent 调用 `dispatch_subtask` 后不阻塞，收到 ticket，通过 notification queue 异步收结果。
- G3：parent 有 pending subtask 且无新工具调用时进入 Waiting，child 完成后自动唤醒回 OnIt。
- G4：child 是独立 Task，有自己的 MAX_TURNS、Evidence Gate、生命周期。child 的状态变化通过 heartbeat 可见于 Orchestrator。

**Non-Goals:**
- N1：不实现 child 的 continuation chain（child 不参与 `/continue` 的 ralph loop）。child 是一次性任务。
- N2：不实现 child 的 child（嵌套 sub-agent 暂不支持，`delegation_depth` 检查阻止）。
- N3：不修改 TUI 渲染（child 的 heartbeat 已经走 Orchestrator 聚合 channel，TUI 自动收到）。
- N4：不实现 sub-agent 的成本隔离（child 消耗 parent 的 provider 配额，不单独计费）。

## Decisions

### D1: Orchestrator 持有 dispatch queue + notification queue map

**决策**：Orchestrator 新增两个字段：
```rust
subtask_request_queue: Arc<Mutex<Vec<SubTaskRequest>>>,  // DispatchSubtaskTool 入队
subtask_notifications: HashMap<String, SubTaskNotificationQueue>,  // parent_task_id → queue
```

`DispatchSubtaskTool::new()` 接收 `subtask_request_queue` 和 `parent_task_id`（从 Task 上下文获取）。execute 时入队 request。

Orchestrator 的 poll loop 新增 `drain_subtask_requests()`：
1. 取出每个 `SubTaskRequest`
2. 调 `dispatch_task(description, focus, [], Some(parent_id))` 创建 child Task
3. 创建 `SubTaskNotificationQueue` 关联到 parent_task_id
4. 通过 parent 的 steering channel 注入"子任务 task-X 已派发"

**理由**：Orchestrator 是 Task 生命周期的唯一管理者。把 child 创建放在 Orchestrator 而非 run loop 内，确保 child 进入 Registry、heartbeat 聚合、`/subtasks` 查询全部自然工作。

**替代方案（已否决）**：parent run loop 直接调 `orchestrator.dispatch_task()` — 否决，因为 run loop 在 `tokio::spawn` 内，持有 Orchestrator 的 `Arc<Mutex>` 会在 async 上下文中死锁。

### D2: Notification queue 轮询 + Waiting 状态

**决策**：parent 的 run loop 在每轮 `process_steering()` 之后新增 `drain_subtask_notifications()`：
1. 检查关联的 notification queue
2. 如果有完成的 notification，注入 `Message::user("[Sub-task result] ...")` 到 active
3. 如果所有 pending subtask 都完成且本轮无新工具调用 → transition 回 OnIt 继续
4. 如果有 pending subtask 且本轮无新工具调用 → transition 到 Waiting

**Waiting 的唤醒**：child 完成后，Orchestrator 通过 parent 的 steering channel 发送 `SteeringMessage::Inject(Message::user("[Sub-task result] ..."))`。parent 收到 steering 后 `drain_followups()` 把消息塞入 active，transition 回 OnIt。

**理由**：复用已有的 `Waiting` 状态和 `steering` 机制，不新增状态。Waiting 已经在 `request_clarification` 场景中验证过。

**替代方案（已否决）**：新增 `WaitingForSubtask` 状态 — 否决，增加状态机复杂度且与 Waiting 语义重叠。

### D3: DispatchSubtaskTool 改为返回 ticket

**决策**：`DispatchSubtaskTool::execute()` 改为：
1. 验证 depth/parallel 限制
2. 入队 `SubTaskRequest`（已有逻辑）
3. 返回 `ToolResult { content: "Sub-task dispatched (ticket: subtask-N). You will be notified.", is_error: false }`

parent LLM 收到"已派发"消息后继续做别的事，或在没有其他工具可调时自然结束本轮 → 进入 Waiting。

**理由**：这是 OpenCode 的 `task()` 工具的模式——dispatch 后不阻塞，结果异步到达。

### D4: parent_task_id 从 ToolContext 获取

**决策**：`ToolContext` 新增 `task_id: Option<String>` 字段。`dispatch_task` 创建 Task 时设置 `task_id`。`DispatchSubtaskTool::execute()` 从 `ctx.task_id` 获取 parent ID，填入 `SubTaskRequest.parent_id`。

**理由**：当前 `SubTaskRequest.parent_id` 恒为空串（bug）。通过 ToolContext 传递是最自然的路径。

**替代方案（已否决）**：给 `DispatchSubtaskTool::new()` 传 parent_id — 否决，因为 tool 在 `Task::new()` 时注册，那时 task_id 可能还未确定。

### D5: child 完成通知走 Orchestrator → parent steering

**决策**：child Task 完成后（Delivered/Stuck/Error），Orchestrator 的 `check_continuations` 或新的 `check_subtask_completions` 检测到 child 的终态：
1. 构造 `SubTaskNotification::Completed { sub_task_id, result, verdict }`
2. 推入 parent 的 `subtask_notifications` queue
3. 通过 parent 的 steering channel 发 `SteeringMessage::Inject(Message::user("[Sub-task result] ..."))`

**理由**：Orchestrator 是唯一能安全访问 parent steering channel 的地方（通过 `inject_message`）。run loop 内的轮询只是读取 notification queue 内容，不直接操作 steering。

## Risks / Trade-offs

### R1: parent Waiting 期间 Orchestrator 可能重复注入

**风险**：多个 child 同时完成时，Orchestrator 可能连续注入多条 steering message。

**缓解**：每次注入前检查 parent 是否已在 Waiting（通过 latest_heartbeat）。Waiting 状态下首次注入唤醒 parent，后续 notification 在 parent 被唤醒后由 run loop 的 `drain_subtask_notifications()` 批量处理。

### R2: child 长时间运行导致 parent 永久 Waiting

**风险**：child 跑满 50 turn 或卡在 Stuck，parent 一直 Waiting。

**缓解**：parent 的 Waiting 有超时机制（复用 `request_clarification` 的 `waiting_since` 时间戳，默认 5 分钟超时）。超时后 parent transition 到 OnIt，注入 "Sub-task timed out" 消息继续执行。

### R3: notification queue 无限增长

**风险**：如果 parent 从不 drain notification（例如 parent 被 cancel），queue 堆积。

**缓解**：queue 有容量上限（10 条），超限丢弃最旧的。cancel 时 queue 随 parent 的 TaskEntry 一起 drop。

## Migration Plan

### Phase 1: Orchestrator dispatch_subtask + Registry 注册
1. Orchestrator 新增 `subtask_request_queue` 和 `subtask_notifications`
2. Orchestrator 新增 `drain_subtask_requests()` → 创建 child Task + 注册
3. Orchestrator 新增 `check_subtask_completions()` → 检测 child 终态 + 注入 parent steering

### Phase 2: Run loop 异步化
4. ToolContext 新增 `task_id` 字段
5. DispatchSubtaskTool 从 ctx 获取 parent_id，修复空串 bug
6. run.rs 移除 `drain_and_run_subtasks()` 同步执行
7. run.rs 新增 `drain_subtask_notifications()` 轮询
8. run.rs Waiting-for-subtask 状态管理

### Phase 3: 测试
9. 单元测试：dispatch_subtask 入队 → Orchestrator drain → child spawn
10. 集成测试：parent dispatch → child run → notification 回传 → parent 唤醒
11. E2E：多 child 并行 → 全部完成 → parent 恢复

**回滚**：Phase 1 和 Phase 2 可独立回滚。Phase 1 回滚后 sub-agent 回到同步模式（仍可用但不推荐）。Phase 2 回滚后 DispatchSubtaskTool 行为不变（返回 ticket 但 parent 会立即进入 Waiting 而非阻塞）。

## Open Questions

1. **child 的 workspace 策略**：Inherited 模式下 child 直接读写 parent 的 workspace——如果 parent 和 child 同时写同一文件怎么办？当前同步模式下不存在这个问题（串行执行），异步后需要考虑。MVP 方案：Inherited 模式仍创建 workspace 副本（Copy-on-Write），child 修改不影响 parent。

2. **child heartbeat 聚合到 parent**：child 的 heartbeat 是否应该聚合到 parent 的 heartbeat 里（如 `subtasks: [{id, status, progress}]`）？`aggregation.rs` 已有纯函数但未接线。MVP 可以先不做聚合，child 的 heartbeat 直接出现在 Orchestrator 的 Task Registry 里。

3. **child 深度限制生效点**：`DispatchLimits` 的 `current_depth` 在 `DispatchSubtaskTool::execute()` 中检查，但 depth 信息存储在 Task 的 `delegation_depth` 字段。child 创建时需要从 parent 继承 depth+1。目前 `with_delegation_depth()` 存在但 Orchestrator 的 `dispatch_subtask` 没调用它。需要在 child 创建后设置。
