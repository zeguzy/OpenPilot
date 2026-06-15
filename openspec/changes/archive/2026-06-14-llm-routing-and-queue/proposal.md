## Why

Three issues blocked real usage: (1) keyword-based routing failed for Chinese messages (almost all went to Foreground), (2) LLM responses were not streamed, (3) no message queue during LLM processing. Additionally the [OPCA_DISPATCH] routing marker was visible to users and the dispatch path had a double-dispatch bug.

## What Changes

- **路由改为 LLM 驱动** — 删除关键词路由 (`route()`)，所有消息发给 LLM。LLM 在回复中用 `OPCA_DISPATCH: <description>` 前缀标记需要后台处理的任务。stream_foreground 检测到前缀后自动触发 dispatch。
- **消息排队** — `is_working` 时 Enter 将消息放入 `pending_messages` 队列。LLM 完成后自动发送队列中的下一条。TUI 显示 `N queued - next: preview`。
- **流式缓冲过滤** — stream_foreground 缓冲第一行不推送，检查是否含 `OPCA_DISPATCH:`。非 dispatch 消息在第一行完成或 Done 时冲刷缓冲。对用户完全隐藏路由标记。
- **修复双 dispatch** — 移除 stream_foreground 里多余的 `dispatch_task` spawn，dispatch 只由 poll_stream 的 Dispatch 事件触发。
- **路由标记对用户隐藏** — `OPCA_DISPATCH:` 前缀行不推送到 TUI 的 Delta，用户只看到友好回复。

## Capabilities

### Modified Capabilities

- `orchestrator-core`: 路由从关键词改为 LLM 驱动
- `cli-frontend`: OrchestratorApi 加 stream_foreground，消息排队
- `tui-interface`: 流式缓冲过滤，排队队列显示，dispatch 事件处理

## Impact

- `crates/opca-core/src/orchestrator/routing.rs` — 加中文关键词（保留但不再被 real.rs 使用）
- `crates/opca-core/src/provider/prompts.rs` — 重写 orchestrator prompt（OPCA_DISPATCH 指令）
- `crates/opca-cli/src/real.rs` — 删除 route() 调用，stream_foreground 加缓冲+dispatch 检测
- `crates/opca-cli/src/tui/app.rs` — pending_messages 队列，Dispatch 事件处理
- `crates/opca-cli/src/main.rs` — Enter 时排队，Tick 时自动发送
- `crates/opca-cli/src/tui/render.rs` — 排队队列行渲染
