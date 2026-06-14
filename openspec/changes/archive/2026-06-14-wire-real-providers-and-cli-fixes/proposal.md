## Why

The initial implementation (create-background-code-agent) delivered all 13 core modules with 600+ tests, but the CLI binary had three gaps that prevented real usage: (1) non-mock mode used a `StubProvider` that returned canned text instead of calling a real LLM, (2) no provider presets for common OpenAI-compatible endpoints like Zhipu/DeepSeek/Ollama, and (3) the REPL could not shut down gracefully. These fixes make `opca` actually usable with a real LLM provider.

## What Changes

- **新增：Provider 预设表** (`provider/presets.rs`) — 10 个内置预设 (zhipu, deepseek, ollama, moonshot, openrouter, groq, mistral, anthropic, openai, gemini)，每个含 base_url + env_key + api protocol。借鉴 pi_agent_rust 的 `provider_metadata.rs` 三层分类 (BuiltInNative / OpenAICompatiblePreset / Custom)。`resolve()` 支持名称+别名查找（如 `glm` → `zhipu`），`normalize_chat_completions_url()` 自动补全 `/chat/completions` 路径。
- **新增：OpenAIProvider base_url 支持** — `OpenAIProvider::with_base_url(api_key, model, base_url)` 构造器，支持任意 OpenAI 兼容端点。原 `::new()` 保持向后兼容。
- **新增：Config 解析** (`config.rs`) — 读取 `.agent/config.toml`，支持 `[model] default`、`[provider] kind`、`[provider] base_url`。Model 解析链：`--model` CLI > `OPCA_MODEL` env > config.toml > 默认。
- **新增：main.rs 真实 Provider 接线** — 删除 `StubProvider`，`create_provider()` 根据 config/env/预设自动构造正确的 Provider。`--provider <KIND>` CLI flag 可覆盖。缺 API key 时给出清晰错误提示。
- **新增：系统提示词** (`provider/prompts.rs`) — Orchestrator 前台回复现在带身份角色 system prompt；Task 系统提示词增加了角色定义 + Focus Contract 组合。
- **修复：优雅关闭** — `/quit` 后进程不再挂住（main.rs 调 `runtime.shutdown()` abort 阻塞线程）；Ctrl+C 直接退出 reedline 输入循环而非发空消息。

## Capabilities

### New Capabilities

无新 capability — 都是 provider-abstraction 和 cli-frontend 的增量改进。

### Modified Capabilities

- `provider-abstraction`: 新增 provider presets、base_url 支持、config 解析、系统提示词
- `cli-frontend`: 新增 `--provider` flag、优雅关闭、真实 LLM 接线

## Impact

### 代码
- `crates/opca-core/src/provider/presets.rs` (新)
- `crates/opca-core/src/provider/prompts.rs` (新)
- `crates/opca-core/src/provider/openai.rs` (改: base_url 字段)
- `crates/opca-core/src/provider/mod.rs` (改: 新增模块声明)
- `crates/opca-core/src/config.rs` (新)
- `crates/opca-core/src/task/task.rs` (改: system prompt 组合)
- `crates/opca-cli/src/main.rs` (改: 删 StubProvider, create_provider, config 读取, shutdown)
- `crates/opca-cli/src/real.rs` (改: query_llm system prompt)
- `crates/opca-cli/src/repl.rs` (改: Ctrl+C 退出)
- `.agent/config.toml` (新: zhipu 配置)

### 依赖
- `toml = "0.8"` 新增到 workspace dependencies

### 配置
- 用户需要设置环境变量（如 `ZHIPU_API_KEY`）而非在 config.toml 中明文写 key
- `.agent/config.toml` 新增 `[provider] kind` 和 `[model] default` 字段
