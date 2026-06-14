## Why

The current CLI uses reedline (line-based REPL) with plain-text output and no streaming. The experience is far behind Claude Code: no Markdown rendering, no streaming (responses appear all at once after a long wait), no token/cost tracking, and no way to see what background Tasks are doing in real time. A full ratatui-based TUI rewrite is needed to deliver a modern, polished experience.

## What Changes

- **新增：ratatui 全屏 TUI** — 替换 reedline，用 ratatui + crossterm 构建全屏界面，包含：状态栏（model/token/cost）、对话区（可滚动）、任务栏（活跃 Task 指示器）、输入区（tui-textarea 多行编辑）。
- **新增：流式输出** — LLM 回复逐字显示，不再等待完整响应。行级 Markdown 渲染（方案 B）：每行完成后立即渲染为格式化文本。
- **新增：折叠面板（Task Panel）** — Task 派发时在对话流中插入可折叠面板，默认折叠，显示状态+进度+摘要。展开后显示 Task 的实时输出流（LLM 文本、tool calls、highlights）。用 `/expand` `/collapse` 或 Enter 键控制。
- **新增：Task 直接对话模式** — `/task <id>` 进入 Task 的 steering 模式，用户可以直接向 Task 发消息（走 steering channel），看到 Task 实时输出。`/back` 返回 Orchestrator 模式。
- **新增：斜杠命令** — 对齐 Claude Code：`/model`（运行时切换模型）、`/compact`（手动压缩上下文）、`/clear`（清空对话）、`/cost`（显示消耗）。
- **新增：Token/Cost 统计** — Provider 解析 SSE 中的 usage 字段，TUI 状态栏实时显示累计 token 数和费用估算。
- **核心改动：Task output_tx channel** — Task agent loop 的每个事件（TextDelta、ToolCall、Highlight）推到 output channel，TUI 订阅实时显示。

## Capabilities

### New Capabilities

- `tui-interface`: 全屏 ratatui TUI，含状态栏、对话区、任务栏、输入区、Markdown 渲染、流式输出、折叠面板、模式切换。

### Modified Capabilities

- `cli-frontend`: 新增 /task /back /model /compact /clear /cost 命令；删除 reedline REPL，替换为 TUI。
- `provider-abstraction`: Provider SSE 响应解析 usage 字段，通过 ProviderEvent::Usage 暴露。
- `task-lifecycle`: Task 新增 output_tx channel，agent loop 事件实时推送。

## Impact

### 代码
- **删除**：`crates/opca-cli/src/repl.rs`（reedline 版 REPL）
- **新增**：`crates/opca-cli/src/tui/`（整个 TUI 层：app/event/render/chat/tasks/input/status/markdown/streaming）
- **改**：`crates/opca-cli/src/main.rs`（启动 TUI 而非 reedline）
- **改**：`crates/opca-cli/src/real.rs`（query_llm 改为推 channel 流式）
- **改**：`crates/opca-cli/src/commands.rs`（新增命令）
- **改**：`crates/opca-core/src/task/run.rs`（加 output_tx 推送）
- **改**：`crates/opca-core/src/task/task.rs`（加 output_tx 字段）
- **改**：`crates/opca-core/src/provider/`（ProviderEvent 加 Usage 变体）

### 依赖
- `ratatui = "0.29"`
- `crossterm = "0.28"`
- `tui-textarea = "0.7"`
- `syntect = "5"`
- `markdown = "1.0"`
