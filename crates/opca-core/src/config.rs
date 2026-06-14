//! Runtime configuration loaded from `.agent/config.toml`.
//!
//! Precedence (highest first): CLI flags > environment variables > this file.

use std::path::Path;

use serde::Deserialize;

/// Top-level config file shape.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub model: ModelConfig,
    #[serde(default)]
    pub provider: ProviderConfig,
}

/// `[model]` section.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ModelConfig {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub audit_model: Option<String>,
}

/// `[provider]` section.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

impl Config {
    /// Load `.agent/config.toml` from `project_root`, or return
    /// [`Config::default`] if the file is missing or unparseable.
    #[must_use]
    pub fn load(project_root: &Path) -> Self {
        let path = project_root.join(".agent").join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_model() {
        let cfg = Config::default();
        assert!(cfg.model.default.is_none());
        assert!(cfg.model.audit_model.is_none());
        assert!(cfg.provider.kind.is_none());
        assert!(cfg.provider.base_url.is_none());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[model]
default = "glm-5.2"
audit_model = "glm-4-flash"

[provider]
kind = "zhipu"
base_url = "https://open.bigmodel.cn/api/paas/v4"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.model.default.as_deref(), Some("glm-5.2"));
        assert_eq!(cfg.model.audit_model.as_deref(), Some("glm-4-flash"));
        assert_eq!(cfg.provider.kind.as_deref(), Some("zhipu"));
        assert_eq!(
            cfg.provider.base_url.as_deref(),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
    }

    #[test]
    fn parse_partial_config() {
        let toml = r#"
[model]
default = "claude-sonnet-4-20250514"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.model.default.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert!(cfg.model.audit_model.is_none());
        assert!(cfg.provider.kind.is_none());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(dir.path());
        assert!(cfg.model.default.is_none());
    }

    #[test]
    fn load_malformed_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join(".agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("config.toml"), "not = valid toml = {{{").unwrap();
        let cfg = Config::load(dir.path());
        assert!(cfg.model.default.is_none());
    }

    #[test]
    fn load_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join(".agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("config.toml"),
            r#"[model]
default = "glm-5.2"
[provider]
kind = "zhipu"
"#,
        )
        .unwrap();
        let cfg = Config::load(dir.path());
        assert_eq!(cfg.model.default.as_deref(), Some("glm-5.2"));
        assert_eq!(cfg.provider.kind.as_deref(), Some("zhipu"));
    }
}
