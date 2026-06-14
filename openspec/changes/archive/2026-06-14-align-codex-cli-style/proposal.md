## Why

The TUI layout after the full-tui-rewrite used a traditional top status bar + bordered input + prefixed chat lines. Researching OpenAI Codex CLI (openai/codex, Rust TUI) revealed a cleaner, more modern design pattern: no top bar, left gutter indentation, minimal input, spinner-based working indicator. Aligning to Codex's visual language improves perceived quality and reduces visual noise.

## What Changes

- **移除顶部状态栏** — model/token/cost 信息从顶部移到底部区域，Codex CLI 不使用顶部栏
- **用户消息视觉区分** — 用户消息用 Gray 色调，不再用 Green "you:" 前缀；AI 消息无前缀
- **左侧 2 列 gutter 缩进** — 所有对话内容左对齐到 2 列缩进，视觉整齐
- **Working 状态指示器** — spinner (⠋⠙⠹...) + "Thinking (Ns - esc to interrupt)" 动画行，工作时显示在输入区上方
- **Esc 中断** — 工作中按 Esc 停止 waiting
- **极简输入区** — 无边框，Cyan 色 "> " 或 "task-0> " 前缀
- **spinner 动画** — 每个 tick 推进 spinner 帧，10 帧 braille 动画

## Capabilities

### Modified Capabilities

- `tui-interface`: 布局改为 Codex 风格（无顶部栏、gutter、spinner、极简输入）

## Impact

- `crates/opca-cli/src/tui/render.rs` — 完全重写渲染逻辑
- `crates/opca-cli/src/tui/app.rs` — 新增 is_working/working_start/spinner_frame 字段和方法
- `crates/opca-cli/src/tui/input.rs` — 新增 cursor() 方法
- `crates/opca-cli/src/main.rs` — run_tui 加 spinner 推进 + Esc 中断
