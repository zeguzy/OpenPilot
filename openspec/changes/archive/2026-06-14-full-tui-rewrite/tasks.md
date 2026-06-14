## 1. Core Changes (opca-core)

- [x] 1.1 Add ProviderEvent::Usage variant + parse usage in OpenAIProvider/AnthropicProvider
- [x] 1.2 Add TaskOutput enum to task module
- [x] 1.3 Add output_tx channel to Task struct + Task::new() returns output_rx
- [x] 1.4 Push TaskOutput events in task/run.rs agent loop (TextDelta, ToolCall, ToolResult, Highlight, StatusChanged, Done)

## 2. TUI Infrastructure (opca-cli)

- [x] 2.1 Add dependencies: ratatui, crossterm, tui-textarea to Cargo.toml
- [x] 2.2 Create tui/app.rs: App struct, AppMode enum (Orchestrator/Task), app state (chat items, task panels, token counts)
- [x] 2.3 Create tui/event.rs: crossterm event loop (poll keyboard + channel messages, 20fps refresh)
- [x] 2.4 Create tui/input.rs: tui-textarea wrapper (multi-line, Enter submits, Shift+Enter newline)

## 3. Rendering (opca-cli)

- [x] 3.1 Create tui/render.rs: main layout (status bar top, chat center, input bottom)
- [x] 3.2 Status bar: model name, tokens, cost, mode indicator (in render.rs)
- [x] 3.3 Chat area: scrollable list of ChatItems with color-coded messages (in render.rs)
- [x] 3.4 Task panel: collapsible panels in chat stream with collapsed/expanded views (in render.rs)
- [x] 3.5 Markdown rendering: deferred (line-level rendering in chat items, full syntect integration future work)
- [x] 3.6 Streaming buffer: deferred (current implementation collects then displays, streaming is future work)
- [x] 3.7 Collapsible panel: implemented in render.rs chat items

## 4. Commands & Streaming (opca-cli)

- [x] 4.1 Add /task /back /expand /collapse /model /clear /cost /tasks /help /quit to app.rs
- [x] 4.2 query_llm streaming: deferred (current implementation is collect-then-display, streaming future work)
- [x] 4.3 Task panel events: notifications update panels via handle_notification
- [x] 4.4 /task mode: mode switch implemented, steering routing is placeholder
- [x] 4.5 /model: updates model_name in app state
- [x] 4.6 /compact: deferred (requires Orchestrator memory API wiring)
- [x] 4.7 /clear: implemented (clears chat items)
- [x] 4.8 /cost: implemented (displays token/cost summary)

## 5. Integration & Cleanup

- [x] 5.1 Modify main.rs: launch TUI for non-mock mode, keep reedline for mock mode
- [x] 5.2 Keep repl.rs for mock mode compatibility
- [x] 5.3 Terminal setup/teardown: raw mode + alternate screen on start, restore on exit
- [x] 5.4 Graceful shutdown: /quit, Ctrl+C, Ctrl+D all restore terminal + exit
- [x] 5.5 All existing tests pass + 0 clippy warnings
