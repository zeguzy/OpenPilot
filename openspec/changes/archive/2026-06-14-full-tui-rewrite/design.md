## Context

opca's CLI currently uses reedline for line-based input and `println!` for output. This produces a poor experience: LLM responses appear all at once after a 10-30s wait, Markdown is shown as raw text, there's no token/cost visibility, and background Task output is invisible. Claude Code sets the bar with streaming output, syntax-highlighted code blocks, and rich slash commands.

This change replaces the entire CLI frontend with a ratatui-based full-screen TUI, while adding a Task output channel to the core library so the TUI can display real-time Task activity.

## Goals / Non-Goals

**Goals:**
- LLM responses stream token-by-token with line-level Markdown rendering
- Background Tasks show live output in collapsible panels within the chat stream
- `/task <id>` lets the user enter a Task's context and steer it directly
- Status bar shows model name, cumulative tokens, and cost estimate
- Slash commands match Claude Code: /model, /compact, /clear, /cost, /task, /back

**Non-Goals:**
- @file references (@src/main.rs) — future enhancement
- Tab completion for file paths — future enhancement
- Color themes / customization — future enhancement
- Full incremental Markdown parser (we use line-level, not token-level)
- TUI mouse support — keyboard only for now

## Decisions

### D1: ratatui + crossterm + tui-textarea

**Choice**: ratatui 0.29 for rendering, crossterm 0.28 for terminal control, tui-textarea 0.7 for multi-line input.
**Rationale**: ratatui is the most active Rust TUI framework. crossterm is its default backend. tui-textarea provides multi-line editing (Shift+Enter for newline) which reedline cannot do in raw mode.
**Alternative**: Keep reedline + add rendering crate — rejected because reedline conflicts with ratatui's raw mode takeover.

### D2: Line-level Markdown rendering (streaming strategy B)

**Choice**: Buffer incoming tokens by line; render each completed line immediately.
**Rationale**: Full incremental parsing is too complex for this phase. Line-level gives good UX (each line renders as soon as `\n` arrives) without the "flash" of render-after-complete. Code blocks detect open/close fences and apply syntect highlighting to lines inside.

### D3: Collapsible Task panels in chat stream

**Choice**: When a Task is dispatched, insert a `TaskPanel` item into the chat list. Default collapsed (one-line summary). `/expand <id>` or Enter (when focused) expands to show full output stream. Panel updates in real-time via channel.
**Rationale**: Keeps the main conversation clean while making Task activity visible and accessible. This is opca's differentiation — Claude Code has no equivalent.

### D4: Task steering mode via /task command

**Choice**: `/task <id>` switches the TUI to Task mode. Input goes to the Task's steering channel as `SteeringMessage::Inject`. Task output streams to the chat area. `/back` returns to Orchestrator mode.
**Rationale**: This lets users directly interact with a Task — give it new instructions, see its reasoning, course-correct in real time. The steering channel already exists in the architecture; the TUI just exposes it.

### D5: ProviderEvent::Usage for token/cost tracking

**Choice**: Add `ProviderEvent::Usage { prompt_tokens, completion_tokens }` variant. Provider implementations parse the `usage` field from SSE done events.
**Rationale**: Token tracking must come from the provider response. Adding it to the event stream is the natural place. The TUI accumulates counts and displays in the status bar.

### D6: App state machine (Orchestrator mode ↔ Task mode)

```
enum AppMode {
    Orchestrator,           // talk to main brain
    Task { task_id: String }, // steer a specific task
}

Transitions:
  Orchestrator --/task <id>--> Task
  Task --/back--> Orchestrator
```

Input routing depends on mode: Orchestrator mode → `handle_message()`, Task mode → `steering_tx.send(Inject(msg))`.

## Risks / Trade-offs

### [Risk] ratatui terminal compatibility
Some terminals may not support the full-screen raw mode correctly.
→ **Mitigation**: Use crossterm (widely compatible). Graceful degradation: if raw mode fails, fall back to line mode with a clear error message.

### [Risk] Markdown rendering complexity
Line-level rendering needs to track context (inside code block? list? table?).
→ **Mitigation**: Maintain a `RenderContext` state struct that tracks block state across lines. Start simple: code blocks, headers, bold/inline code. Tables and nested lists can come later.

### [Risk] Performance with many Task events
A long-running Task could accumulate thousands of events, making the panel slow to render.
→ **Mitigation**: Cap the event buffer at 500 lines; older lines are truncated with a "..." indicator. The full log is in the session JSONL.

### [Trade-off] No mouse support
Keyboard-only navigation. Users can't click to expand/collapse panels.
→ **Accept**: Keyboard shortcuts (Enter on focused panel) work well. Mouse can be added later via crossterm's mouse events.
