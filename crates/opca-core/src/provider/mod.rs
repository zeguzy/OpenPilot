pub mod anthropic;
pub mod context;
pub mod gemini;
pub mod message;
pub mod openai;
pub mod presets;
pub mod prompts;
#[allow(clippy::module_inception)]
pub mod provider;
pub mod tool;

pub use anthropic::AnthropicProvider;
pub use anyhow;
pub use context::{ContextBuilder, ContextOwned, ContextRef};
pub use gemini::GeminiProvider;
pub use message::{Message, MessageRole};
pub use openai::OpenAIProvider;
pub use presets::{ApiProtocol, PRESETS, ProviderPreset, normalize_chat_completions_url, resolve};
pub use prompts::{orchestrator_prompt, task_prompt};
pub use provider::{Provider, ProviderEvent, ProviderStream, StopReason};
pub use tool::{ToolCall, ToolDef, ToolEffects, ToolResult};
