use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoConfig {
    #[serde(default = "default_true", alias = "auto_enabled")]
    pub enabled: bool,
    #[serde(
        default = "default_size_threshold_bytes",
        alias = "token_threshold_bytes"
    )]
    pub size_threshold_bytes: u64,
    #[serde(default = "default_tool_burst_threshold")]
    pub tool_burst_threshold: usize,
    #[serde(default = "default_cooldown_seconds")]
    pub cooldown_seconds: i64,
    #[serde(default = "default_growth_delta_bytes")]
    pub growth_delta_bytes: u64,
}

impl Default for AutoConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            size_threshold_bytes: default_size_threshold_bytes(),
            tool_burst_threshold: default_tool_burst_threshold(),
            cooldown_seconds: default_cooldown_seconds(),
            growth_delta_bytes: default_growth_delta_bytes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionConfig {
    #[serde(default = "default_recent_user_turns")]
    pub recent_user_turns: usize,
    #[serde(default = "default_recent_tools_cap")]
    pub recent_tools_cap: usize,
    #[serde(default = "default_recent_errors_cap")]
    pub recent_errors_cap: usize,
    #[serde(default = "default_recent_window_steps")]
    pub recent_window_steps: usize,
    #[serde(default = "default_artifact_retention_count")]
    pub artifact_retention_count: usize,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            recent_user_turns: default_recent_user_turns(),
            recent_tools_cap: default_recent_tools_cap(),
            recent_errors_cap: default_recent_errors_cap(),
            recent_window_steps: default_recent_window_steps(),
            artifact_retention_count: default_artifact_retention_count(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyConfig {
    #[serde(default = "default_false")]
    pub redact_secrets: bool,
    #[serde(default = "default_true")]
    pub restrict_permissions: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            redact_secrets: default_false(),
            restrict_permissions: default_true(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_size_threshold_bytes() -> u64 {
    264_000
}
fn default_tool_burst_threshold() -> usize {
    20
}
fn default_cooldown_seconds() -> i64 {
    180
}
fn default_growth_delta_bytes() -> u64 {
    25_600
}
fn default_recent_user_turns() -> usize {
    10
}
fn default_recent_tools_cap() -> usize {
    20
}
fn default_recent_errors_cap() -> usize {
    30
}
fn default_recent_window_steps() -> usize {
    6
}
fn default_artifact_retention_count() -> usize {
    20
}
fn default_log_level() -> String {
    "info".to_string()
}

fn default_deep_after_user_turns() -> usize {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShakeCoreConfig {
    #[serde(default = "default_recent_user_turns")]
    pub keep_recent_turns: usize,
    #[serde(default = "default_recent_tools_cap")]
    pub keep_recent_tools: usize,
    #[serde(default = "default_recent_errors_cap")]
    pub keep_recent_errors: usize,
    #[serde(default = "default_deep_after_user_turns")]
    pub deep_after_user_turns: usize,
    #[serde(default = "default_false")]
    pub redact_secrets: bool,
}

impl Default for ShakeCoreConfig {
    fn default() -> Self {
        Self {
            keep_recent_turns: default_recent_user_turns(),
            keep_recent_tools: default_recent_tools_cap(),
            keep_recent_errors: default_recent_errors_cap(),
            deep_after_user_turns: default_deep_after_user_turns(),
            redact_secrets: default_false(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShakeConfig {
    #[serde(default)]
    pub shake: Option<ShakeCoreConfig>,
    #[serde(default, alias = "advanced")]
    pub auto: AutoConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
    #[serde(default = "default_deep_after_user_turns")]
    pub deep_after_user_turns: usize,
}

impl Default for ShakeConfig {
    fn default() -> Self {
        Self {
            shake: None,
            auto: AutoConfig::default(),
            retention: RetentionConfig::default(),
            privacy: PrivacyConfig::default(),
            diagnostics: DiagnosticsConfig::default(),
            deep_after_user_turns: default_deep_after_user_turns(),
        }
    }
}

impl ShakeConfig {
    /// Normalizes configuration by syncing [shake] core table values into retention/privacy.
    pub fn normalize(&mut self) {
        if let Some(core) = &self.shake {
            self.retention.recent_user_turns = core.keep_recent_turns;
            self.retention.recent_tools_cap = core.keep_recent_tools;
            self.retention.recent_errors_cap = core.keep_recent_errors;
            self.deep_after_user_turns = core.deep_after_user_turns;
            self.privacy.redact_secrets = core.redact_secrets;
        }
    }

    /// Resolves the default global config file path: `~/.gemini/config/shake.toml`.
    pub fn global_config_path() -> Option<PathBuf> {
        let home = env::var("HOME").or_else(|_| env::var("USERPROFILE")).ok()?;
        if home.is_empty() {
            return None;
        }
        Some(Path::new(&home).join(".gemini/config/shake.toml"))
    }

    /// Loads configuration by reading `~/.gemini/config/shake.toml` (if present)
    /// and applying any overriding environment variables.
    pub fn load() -> Self {
        let mut config = Self::default();

        if let Some(path) = Self::global_config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    match toml::from_str::<ShakeConfig>(&content) {
                        Ok(mut parsed) => {
                            parsed.normalize();
                            config = parsed;
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to parse shake config file '{}': {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
            }
        }

        config.apply_env_overrides();
        config
    }

    /// Loads configuration from a specific path, then applies environment variables.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path.display(), e))?;
        let mut config: ShakeConfig = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path.display(), e))?;
        config.normalize();
        config.apply_env_overrides();
        Ok(config)
    }

    /// Overrides configuration parameters from environment variables (P0-4).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = env::var("SHAKE_AUTO_DISABLE").or_else(|_| env::var("SHAKE_DISABLED")) {
            let v = val.trim().to_lowercase();
            if v == "1" || v == "true" || v == "yes" {
                self.auto.enabled = false;
            } else if v == "0" || v == "false" || v == "no" {
                self.auto.enabled = true;
            }
        }

        if let Ok(val) = env::var("SHAKE_AUTO_ENABLE") {
            let v = val.trim().to_lowercase();
            if v == "1" || v == "true" || v == "yes" {
                self.auto.enabled = true;
            }
        }

        if let Ok(val) =
            env::var("SHAKE_KEEP_RECENT_TURNS").or_else(|_| env::var("SHAKE_RECENT_USER_TURNS"))
        {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.retention.recent_user_turns = parsed;
            }
        }

        if let Ok(val) =
            env::var("SHAKE_KEEP_RECENT_TOOLS").or_else(|_| env::var("SHAKE_TOOLS_CAP"))
        {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.retention.recent_tools_cap = parsed;
            }
        }

        if let Ok(val) =
            env::var("SHAKE_KEEP_RECENT_ERRORS").or_else(|_| env::var("SHAKE_ERRORS_CAP"))
        {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.retention.recent_errors_cap = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_DEEP_AFTER_TURNS") {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.deep_after_user_turns = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_RECENT_WINDOW") {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.retention.recent_window_steps = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_TOKEN_THRESHOLD_BYTES") {
            if let Ok(parsed) = val.trim().parse::<u64>() {
                self.auto.size_threshold_bytes = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_TOOL_BURST_THRESHOLD") {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.auto.tool_burst_threshold = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_COOLDOWN_SECONDS") {
            if let Ok(parsed) = val.trim().parse::<i64>() {
                self.auto.cooldown_seconds = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_GROWTH_DELTA_BYTES") {
            if let Ok(parsed) = val.trim().parse::<u64>() {
                self.auto.growth_delta_bytes = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_SECRET_REDACTION") {
            let v = val.trim().to_lowercase();
            if v == "1" || v == "true" || v == "yes" {
                self.privacy.redact_secrets = true;
            } else if v == "0" || v == "false" || v == "no" {
                self.privacy.redact_secrets = false;
            }
        }

        if let Ok(val) = env::var("SHAKE_ARTIFACT_RETENTION") {
            if let Ok(parsed) = val.trim().parse::<usize>() {
                self.retention.artifact_retention_count = parsed;
            }
        }

        if let Ok(val) = env::var("SHAKE_LOG_LEVEL") {
            let v = val.trim();
            if !v.is_empty() {
                self.diagnostics.log_level = v.to_lowercase();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ShakeConfig::default();
        assert!(config.auto.enabled);
        assert_eq!(config.auto.size_threshold_bytes, 264_000);
        assert_eq!(config.auto.tool_burst_threshold, 20);
        assert_eq!(config.auto.cooldown_seconds, 180);
        assert_eq!(config.auto.growth_delta_bytes, 25_600);
        assert_eq!(config.retention.recent_user_turns, 10);
        assert_eq!(config.retention.recent_tools_cap, 20);
        assert_eq!(config.retention.recent_errors_cap, 30);
        assert_eq!(config.retention.recent_window_steps, 6);
        assert_eq!(config.retention.artifact_retention_count, 20);
        assert!(!config.privacy.redact_secrets);
        assert!(config.privacy.restrict_permissions);
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
        [auto]
        enabled = false
        size_threshold_bytes = 100000
        tool_burst_threshold = 15
        cooldown_seconds = 60
        growth_delta_bytes = 10000

        [retention]
        recent_user_turns = 5
        recent_tools_cap = 10
        recent_errors_cap = 15

        [privacy]
        redact_secrets = true

        [diagnostics]
        log_level = "debug"
        "#;

        let parsed: ShakeConfig = toml::from_str(toml_str).unwrap();
        assert!(!parsed.auto.enabled);
        assert_eq!(parsed.auto.size_threshold_bytes, 100_000);
        assert_eq!(parsed.auto.tool_burst_threshold, 15);
        assert_eq!(parsed.auto.cooldown_seconds, 60);
        assert_eq!(parsed.auto.growth_delta_bytes, 10_000);
        assert_eq!(parsed.retention.recent_user_turns, 5);
        assert_eq!(parsed.retention.recent_tools_cap, 10);
        assert_eq!(parsed.retention.recent_errors_cap, 15);
        assert!(parsed.privacy.redact_secrets);
        assert_eq!(parsed.diagnostics.log_level, "debug");
    }

    #[test]
    fn test_token_threshold_bytes_alias() {
        let toml_str = r#"
        [auto]
        token_threshold_bytes = 123456
        "#;

        let parsed: ShakeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.auto.size_threshold_bytes, 123_456);
    }

    #[test]
    fn test_modern_simplified_shake_config() {
        let toml_str = r#"
        [shake]
        keep_recent_turns = 12
        keep_recent_tools = 25
        keep_recent_errors = 35
        deep_after_user_turns = 40
        redact_secrets = true

        [advanced]
        auto_enabled = true
        token_threshold_bytes = 200000
        "#;

        let mut parsed: ShakeConfig = toml::from_str(toml_str).unwrap();
        parsed.normalize();

        assert_eq!(parsed.retention.recent_user_turns, 12);
        assert_eq!(parsed.retention.recent_tools_cap, 25);
        assert_eq!(parsed.retention.recent_errors_cap, 35);
        assert_eq!(parsed.deep_after_user_turns, 40);
        assert!(parsed.privacy.redact_secrets);
        assert!(parsed.auto.enabled);
        assert_eq!(parsed.auto.size_threshold_bytes, 200_000);
    }
}
