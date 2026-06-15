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
    #[serde(default)]
    pub continuation: ContinuationConfig,
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

/// `[continuation]` section.
///
/// Bounds auto-iteration chains. All fields default to conservative
/// engineering values; absence of the section disables continuation.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ContinuationConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_max_total_cost_usd")]
    pub max_total_cost_usd: f64,
    #[serde(default = "default_max_total_duration_minutes")]
    pub max_total_duration_minutes: u64,
    #[serde(default = "default_max_no_progress_rounds")]
    pub max_no_progress_rounds: u32,
    #[serde(default = "default_audit_confidence_threshold")]
    pub audit_confidence_threshold: f64,
}

const fn default_enabled() -> bool {
    false
}
const fn default_max_iterations() -> u32 {
    10
}
const fn default_max_total_cost_usd() -> f64 {
    5.0
}
const fn default_max_total_duration_minutes() -> u64 {
    30
}
const fn default_max_no_progress_rounds() -> u32 {
    2
}
const fn default_audit_confidence_threshold() -> f64 {
    0.5
}

impl Default for ContinuationConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_iterations: default_max_iterations(),
            max_total_cost_usd: default_max_total_cost_usd(),
            max_total_duration_minutes: default_max_total_duration_minutes(),
            max_no_progress_rounds: default_max_no_progress_rounds(),
            audit_confidence_threshold: default_audit_confidence_threshold(),
        }
    }
}

impl ContinuationConfig {
    #[must_use]
    pub const fn max_total_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.max_total_duration_minutes * 60)
    }
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
    fn default_continuation_is_disabled_with_conservative_caps() {
        let cfg = ContinuationConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_iterations, 10);
        assert!((cfg.max_total_cost_usd - 5.0).abs() < f64::EPSILON);
        assert_eq!(cfg.max_total_duration_minutes, 30);
        assert_eq!(cfg.max_no_progress_rounds, 2);
        assert!((cfg.audit_confidence_threshold - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn max_total_duration_converts_minutes_to_seconds() {
        let cfg = ContinuationConfig::default();
        assert_eq!(
            cfg.max_total_duration(),
            std::time::Duration::from_secs(30 * 60)
        );
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
        assert!(!cfg.continuation.enabled);
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
    fn parse_continuation_section() {
        let toml = r"
[continuation]
enabled = true
max_iterations = 7
max_total_cost_usd = 2.5
max_total_duration_minutes = 15
max_no_progress_rounds = 3
audit_confidence_threshold = 0.8
";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.continuation.enabled);
        assert_eq!(cfg.continuation.max_iterations, 7);
        assert!((cfg.continuation.max_total_cost_usd - 2.5).abs() < f64::EPSILON);
        assert_eq!(cfg.continuation.max_total_duration_minutes, 15);
        assert_eq!(cfg.continuation.max_no_progress_rounds, 3);
        assert!((cfg.continuation.audit_confidence_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_continuation_partial_uses_defaults() {
        let toml = r"
[continuation]
enabled = true
";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.continuation.enabled);
        assert_eq!(cfg.continuation.max_iterations, 10);
        assert!((cfg.continuation.max_total_cost_usd - 5.0).abs() < f64::EPSILON);
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

    #[test]
    fn load_file_with_continuation_section() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join(".agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("config.toml"),
            r"[continuation]
enabled = true
max_iterations = 4
",
        )
        .unwrap();
        let cfg = Config::load(dir.path());
        assert!(cfg.continuation.enabled);
        assert_eq!(cfg.continuation.max_iterations, 4);
    }
}
