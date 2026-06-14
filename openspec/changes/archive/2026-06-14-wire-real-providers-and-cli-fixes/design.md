## Context

The initial opca implementation delivered 158 tasks across 18 groups, producing 23,000+ lines of Rust with 600+ tests. However, three gaps prevented real-world usage:

1. **StubProvider placeholder**: `main.rs` used a `StubProvider` that returned canned text instead of calling a real LLM. The real `AnthropicProvider`, `OpenAIProvider`, and `GeminiProvider` were implemented but never wired into the CLI binary.

2. **No provider presets**: Users had to know exact API endpoint URLs. There was no built-in mapping for common OpenAI-compatible providers like Zhipu (智谱), DeepSeek, Ollama, Moonshot, etc.

3. **No graceful shutdown**: `/quit` left the process hanging because `runtime.shutdown()` was never called. Ctrl+C sent an empty string instead of breaking the input loop.

## Goals / Non-Goals

**Goals:**
- `opca` CLI works with a real LLM provider out of the box (no code changes needed)
- Users configure provider via `.agent/config.toml` + environment variable
- Common Chinese LLM providers (Zhipu, DeepSeek) work via presets
- Process exits cleanly on `/quit`, Ctrl+C, Ctrl+D

**Non-Goals:**
- Model registry / models.json (pi_agent_rust style) — future enhancement
- URL normalization heuristics beyond simple suffix append
- Provider alias fuzzy matching ("did you mean?") — future enhancement
- CompatConfig (capability flags, field overrides) — not needed for MVP

## Decisions

### D1: Preset table as static const (not config file)

**Choice**: Hardcoded `&[ProviderPreset]` const array in Rust source.
**Rationale**: Presets rarely change. Compiling them in means zero config overhead for common providers. Users who need custom providers can use `[provider] base_url` in config.toml.
**Alternative considered**: External models.json (like pi_agent_rust) — rejected as over-engineering for 10 presets.

### D2: API key from environment variable, not config file

**Choice**: Keys read from `std::env::var(preset.env_key)`, never from config.toml.
**Rationale**: Security best practice. Config files may be committed to git; env vars stay local. This matches Claude Code, OpenAI CLI, and standard conventions.

### D3: OpenAIProvider reused for all OpenAI-compatible endpoints

**Choice**: Single `OpenAIProvider::with_base_url()` covers Zhipu, DeepSeek, Ollama, etc.
**Rationale**: These providers all implement the same `/v1/chat/completions` SSE protocol. Separate provider implementations would duplicate identical streaming logic.
**Alternative considered**: Separate `ZhipuProvider` class — rejected as unnecessary code duplication.

### D4: System prompts as const fn returning &'static str

**Choice**: `pub const fn orchestrator_prompt() -> &'static str` — compile-time constant, no allocation.
**Rationale**: Prompts are static text that doesn't change at runtime. Const fn is zero-cost.

## Risks / Trade-offs

### [Risk] Preset URLs may change
Provider endpoints may change over time. Hardcoded presets become stale.
→ **Mitigation**: Users can override via `[provider] base_url` in config.toml. Presets can be updated in source with a simple PR.

### [Risk] Model name inference is fragile
`guess_kind_from_model()` uses prefix matching (`glm` → zhipu). New model names may not match.
→ **Mitigation**: Users can always specify `--provider` explicitly. Inference is a convenience, not a requirement.

### [Trade-off] No URL normalization heuristics
`normalize_chat_completions_url()` simply appends `/chat/completions` if missing. It doesn't handle edge cases like `/v1/responses` suffix stripping (which pi_agent_rust does).
→ **Accept**: MVP doesn't need this complexity. The simple append works for all 10 presets.
