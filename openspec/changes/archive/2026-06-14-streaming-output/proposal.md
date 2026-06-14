## Why

LLM responses appeared all at once after a multi-second wait — the TUI collected the entire response before displaying anything. This made the agent feel slow and unresponsive compared to Claude Code and Codex CLI, which stream tokens incrementally. Additionally, pressing Enter caused a visible delay because `handle_message` synchronously called `query_llm` via `block_in_place`, blocking the event loop.

## What Changes

- **新增：流式输出** — LLM 回复逐字显示，不再等待完整响应。`stream_foreground` 方法在独立 tokio task 里异步拉取 Provider stream，每个 TextDelta 通过 channel 推送到 TUI。
- **新增：StreamEvent 枚举** — `Delta(String)` / `Done` / `Error(String)`，TUI 的 `poll_stream` 在每个 tick 消费 channel 更新 `StreamingAssistant` chat item。
- **新增：StreamingAssistant chat item** — 流式过程中的临时 chat item，文本逐步增长；Done 后转换为 `AssistantText`。
- **修复：Enter 延迟** — `handle_message` 不再同步调 LLM。Foreground 路由立即返回空文本，真正的 LLM 调用由 `stream_foreground` 异步执行。
- **改动：task status 查询改为 Acknowledged** — 即时返回的 task status / list 不再走 Foreground（避免触发流式路径）。

## Capabilities

### Modified Capabilities

- `tui-interface`: 流式输出 + StreamingAssistant item + poll_stream
- `cli-frontend`: OrchestratorApi trait 加 stream_foreground 方法

## Impact

- `crates/opca-cli/src/tui/app.rs` — StreamEvent, StreamingAssistant, poll_stream, stream_rx/tx
- `crates/opca-cli/src/lib.rs` — OrchestratorApi trait 加 stream_foreground
- `crates/opca-cli/src/real.rs` — stream_foreground 实现 (spawn async provider stream)
- `crates/opca-cli/src/mock.rs` — stream_foreground mock (逐词推送)
- `crates/opca-cli/src/tui/render.rs` — StreamingAssistant 渲染
- `crates/opca-cli/src/main.rs` — run_tui 加 stream_foreground 调用 + poll_stream on tick
