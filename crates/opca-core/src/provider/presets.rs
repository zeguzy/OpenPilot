//! Provider presets — well-known LLM endpoints with their base URLs,
//! environment variable names, and wire protocols.
//!
//! [`resolve`] looks up a preset by canonical name or alias (case-insensitive).
//! [`normalize_chat_completions_url`] turns a bare base URL into a full
//! `/chat/completions` endpoint for OpenAI-compatible providers.

/// Wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiProtocol {
    /// OpenAI-compatible `/v1/chat/completions` streaming.
    OpenAIChat,
    /// Anthropic Messages API (`/v1/messages`).
    AnthropicMessages,
}

/// A bundled configuration for a well-known LLM provider.
#[derive(Debug, Clone, Copy)]
pub struct ProviderPreset {
    /// Canonical lower-case name.
    pub name: &'static str,
    /// Alternative names the user might type.
    pub aliases: &'static [&'static str],
    /// Base URL **without** the trailing `/chat/completions`.
    pub base_url: &'static str,
    /// Environment variable holding the API key. Empty = no key needed.
    pub env_key: &'static str,
    /// Wire protocol for this provider.
    pub api: ApiProtocol,
}

/// All built-in provider presets.
pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "zhipu",
        aliases: &["glm", "bigmodel", "chatglm"],
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        env_key: "ZHIPU_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "deepseek",
        aliases: &["deep-seek"],
        base_url: "https://api.deepseek.com/v1",
        env_key: "DEEPSEEK_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "ollama",
        aliases: &[],
        base_url: "http://127.0.0.1:11434/v1",
        env_key: "",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "moonshot",
        aliases: &["kimi"],
        base_url: "https://api.moonshot.cn/v1",
        env_key: "MOONSHOT_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "openrouter",
        aliases: &["open-router"],
        base_url: "https://openrouter.ai/api/v1",
        env_key: "OPENROUTER_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "groq",
        aliases: &[],
        base_url: "https://api.groq.com/openai/v1",
        env_key: "GROQ_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "mistral",
        aliases: &[],
        base_url: "https://api.mistral.ai/v1",
        env_key: "MISTRAL_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "anthropic",
        aliases: &["claude"],
        base_url: "https://api.anthropic.com",
        env_key: "ANTHROPIC_API_KEY",
        api: ApiProtocol::AnthropicMessages,
    },
    ProviderPreset {
        name: "openai",
        aliases: &["gpt"],
        base_url: "https://api.openai.com/v1",
        env_key: "OPENAI_API_KEY",
        api: ApiProtocol::OpenAIChat,
    },
    ProviderPreset {
        name: "gemini",
        aliases: &["google"],
        base_url: "https://generativelanguage.googleapis.com",
        env_key: "GEMINI_API_KEY",
        api: ApiProtocol::AnthropicMessages,
    },
];

/// Look up a preset by canonical name or alias (case-insensitive).
#[must_use]
pub fn resolve(name: &str) -> Option<&'static ProviderPreset> {
    let lower = name.to_lowercase();
    PRESETS
        .iter()
        .find(|p| p.name == lower || p.aliases.iter().any(|a| *a == lower))
}

/// Normalise an OpenAI-compatible base URL so it ends with `/chat/completions`.
///
/// Already-complete URLs are returned unchanged; everything else gets the
/// suffix appended after stripping trailing slashes.
#[must_use]
pub fn normalize_chat_completions_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_by_canonical_name() {
        assert_eq!(resolve("zhipu").unwrap().name, "zhipu");
        assert_eq!(resolve("deepseek").unwrap().name, "deepseek");
        assert_eq!(resolve("ollama").unwrap().name, "ollama");
        assert_eq!(resolve("anthropic").unwrap().name, "anthropic");
        assert_eq!(resolve("openai").unwrap().name, "openai");
    }

    #[test]
    fn resolve_is_case_insensitive() {
        assert_eq!(resolve("ZHIPU").unwrap().name, "zhipu");
        assert_eq!(resolve("OpenAI").unwrap().name, "openai");
        assert_eq!(resolve("GROQ").unwrap().name, "groq");
    }

    #[test]
    fn resolve_by_alias() {
        assert_eq!(resolve("glm").unwrap().name, "zhipu");
        assert_eq!(resolve("bigmodel").unwrap().name, "zhipu");
        assert_eq!(resolve("chatglm").unwrap().name, "zhipu");
        assert_eq!(resolve("kimi").unwrap().name, "moonshot");
        assert_eq!(resolve("claude").unwrap().name, "anthropic");
        assert_eq!(resolve("gpt").unwrap().name, "openai");
        assert_eq!(resolve("google").unwrap().name, "gemini");
        assert_eq!(resolve("deep-seek").unwrap().name, "deepseek");
        assert_eq!(resolve("open-router").unwrap().name, "openrouter");
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert!(resolve("nonexistent").is_none());
        assert!(resolve("").is_none());
    }

    #[test]
    fn ollama_has_empty_env_key() {
        let preset = resolve("ollama").unwrap();
        assert!(preset.env_key.is_empty());
    }

    #[test]
    fn all_presets_have_distinct_names() {
        let mut names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let original = names.clone();
        names.dedup();
        assert_eq!(names.len(), original.len(), "duplicate preset names");
    }

    #[test]
    fn normalize_zhipu_base() {
        assert_eq!(
            normalize_chat_completions_url("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn normalize_openai_base() {
        assert_eq!(
            normalize_chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_deepseek_base() {
        assert_eq!(
            normalize_chat_completions_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_already_complete_url() {
        let url = "https://api.openai.com/v1/chat/completions";
        assert_eq!(normalize_chat_completions_url(url), url);
    }

    #[test]
    fn normalize_strips_trailing_slash() {
        assert_eq!(
            normalize_chat_completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_ollama_local() {
        assert_eq!(
            normalize_chat_completions_url("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_already_complete_with_trailing_slash() {
        assert_eq!(
            normalize_chat_completions_url("https://api.openai.com/v1/chat/completions/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
