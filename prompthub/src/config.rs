use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Context;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAuth {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<SourceAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<Source>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = global_config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Cannot read config: {}", config_path.display()))?;
            Self::from_yaml(&content)
                .with_context(|| format!("Cannot parse config YAML: {}", config_path.display()))
        } else {
            Ok(Config::default_config())
        }
    }

    /// Parse a `Config` from a YAML string.  Useful for testing without touching
    /// the global config path on disk.
    pub fn from_yaml(yaml: &str) -> anyhow::Result<Self> {
        serde_yaml::from_str(yaml).map_err(|e| anyhow::anyhow!("{}", e))
    }

    pub fn default_config() -> Self {
        Config {
            sources: vec![
                Source {
                    name: "official".to_string(),
                    url: "https://raw.githubusercontent.com/prompthub/layers/main".to_string(),
                    default: true,
                    auth: None,
                }
            ],
        }
    }

    /// Save config to the default global path (~/.prompthub/config.yaml) with 0600 permissions.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to_path(&global_config_path())
    }

    /// Save config to an explicit path with 0600 permissions.
    /// Separated from `save()` to make permission testing possible without
    /// touching the real ~/.prompthub/config.yaml.
    pub fn save_to_path(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;

        // Restrict config file to owner-only read/write (protects stored tokens).
        // Skipped on non-Unix platforms (Windows uses ACLs instead).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    pub fn default_source(&self) -> Option<&Source> {
        self.sources.iter().find(|s| s.default).or_else(|| self.sources.first())
    }

    /// Find a source whose URL matches `url` (trailing slash is ignored on both sides).
    pub fn find_source_by_url(&self, url: &str) -> Option<&Source> {
        let normalized = url.trim_end_matches('/');
        self.sources.iter().find(|s| s.url.trim_end_matches('/') == normalized)
    }

    /// Mutable version — used by `ph login` and `ph logout`.
    pub fn find_source_by_url_mut(&mut self, url: &str) -> Option<&mut Source> {
        let normalized = url.trim_end_matches('/').to_string();
        self.sources.iter_mut().find(|s| s.url.trim_end_matches('/') == normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_official_source() {
        let cfg = Config::default_config();
        assert_eq!(cfg.sources.len(), 1, "default config should have one source");
        assert_eq!(cfg.sources[0].name, "official");
        assert!(cfg.sources[0].default, "official source should be marked as default");
    }

    #[test]
    fn test_default_source_returns_default_marked_source() {
        let cfg = Config::default_config();
        let src = cfg.default_source().expect("default source should be present");
        assert_eq!(src.name, "official");
    }

    #[test]
    fn test_default_source_falls_back_to_first_when_none_marked() {
        let cfg = Config {
            sources: vec![
                Source { name: "mirror".to_string(), url: "https://mirror.example.com".to_string(), default: false, auth: None },
                Source { name: "secondary".to_string(), url: "https://secondary.example.com".to_string(), default: false, auth: None },
            ],
        };
        let src = cfg.default_source().expect("should fall back to first source");
        assert_eq!(src.name, "mirror", "first source should be used as fallback");
    }

    #[test]
    fn test_from_str_parses_valid_yaml() {
        let yaml = r#"
sources:
  - name: custom
    url: https://example.com/layers
    default: true
"#;
        let cfg = Config::from_yaml(yaml).expect("should parse valid YAML");
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].name, "custom");
        assert_eq!(cfg.sources[0].url, "https://example.com/layers");
        assert!(cfg.sources[0].default);
    }

    #[test]
    fn test_from_str_empty_sources() {
        let yaml = "sources: []\n";
        let cfg = Config::from_yaml(yaml).expect("should parse empty sources");
        assert!(cfg.sources.is_empty());
        assert!(cfg.default_source().is_none(), "no sources means no default source");
    }

    #[test]
    fn test_from_str_invalid_yaml_errors() {
        let yaml = "sources: [invalid: yaml: here\n";
        assert!(Config::from_yaml(yaml).is_err(), "malformed YAML should produce an error");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        use tempfile::TempDir;

        // Build a config with a custom source
        let cfg = Config {
            sources: vec![
                Source {
                    name: "custom".to_string(),
                    url: "https://custom.example.com/layers".to_string(),
                    default: true,
                    auth: None,
                },
                Source {
                    name: "mirror".to_string(),
                    url: "https://mirror.example.com/layers".to_string(),
                    default: false,
                    auth: None,
                },
            ],
        };

        // Serialize to YAML and round-trip through from_yaml
        let yaml = serde_yaml::to_string(&cfg).expect("serialization must succeed");
        let loaded = Config::from_yaml(&yaml).expect("deserialization must succeed");

        assert_eq!(loaded.sources.len(), 2, "both sources should survive roundtrip");
        assert_eq!(loaded.sources[0].name, "custom");
        assert_eq!(loaded.sources[0].url, "https://custom.example.com/layers");
        assert!(loaded.sources[0].default, "default flag should survive roundtrip");
        assert_eq!(loaded.sources[1].name, "mirror");
        assert!(!loaded.sources[1].default, "non-default flag should survive roundtrip");

        // Also verify that writing to disk and reading back produces identical content
        let tmp = TempDir::new().expect("tmpdir creation must succeed");
        let file_path = tmp.path().join("config.yaml");
        std::fs::write(&file_path, &yaml).expect("write must succeed");
        let from_disk = std::fs::read_to_string(&file_path).expect("read must succeed");
        let loaded_from_disk = Config::from_yaml(&from_disk).expect("parse from disk must succeed");
        assert_eq!(loaded_from_disk.sources.len(), 2, "disk roundtrip must preserve source count");
    }

    #[test]
    fn test_source_auth_round_trips_through_yaml() {
        let yaml = r#"
sources:
  - name: my-company
    url: https://registry.example.com
    default: true
    auth:
      token: phrt_abc123
  - name: official
    url: https://raw.githubusercontent.com/prompthub/layers/main
    default: false
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        assert_eq!(cfg.sources.len(), 2);
        let authenticated = &cfg.sources[0];
        assert_eq!(authenticated.auth.as_ref().unwrap().token, "phrt_abc123");
        let unauthenticated = &cfg.sources[1];
        assert!(unauthenticated.auth.is_none());
    }

    #[test]
    fn test_source_without_auth_field_parses_correctly() {
        // Existing config files (pre-auth) must still parse without errors
        let yaml = r#"
sources:
  - name: official
    url: https://raw.githubusercontent.com/prompthub/layers/main
    default: true
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        assert!(cfg.sources[0].auth.is_none());
    }

    #[test]
    fn test_auth_token_not_serialized_when_absent() {
        let cfg = Config {
            sources: vec![
                Source {
                    name: "official".to_string(),
                    url: "https://example.com".to_string(),
                    default: true,
                    auth: None,
                },
            ],
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(!yaml.contains("auth"), "auth key must be absent when None");
    }

    #[test]
    fn test_find_by_url_exact_match() {
        let cfg = Config {
            sources: vec![
                Source {
                    name: "my-reg".to_string(),
                    url: "https://registry.example.com".to_string(),
                    default: true,
                    auth: None,
                },
            ],
        };
        assert!(cfg.find_source_by_url("https://registry.example.com").is_some());
        assert!(cfg.find_source_by_url("https://registry.example.com/").is_some());
        assert!(cfg.find_source_by_url("https://other.example.com").is_none());
    }

    #[test]
    fn test_save_creates_file_with_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.yaml");
        let cfg = Config::default_config();

        // Write using save_to_path
        cfg.save_to_path(&path).unwrap();

        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "config file must be written with 0600 permissions"
        );
    }
}

/// Returns ~/.prompthub/
pub fn global_hub_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".prompthub")
}

/// Returns ~/.prompthub/layers/
pub fn global_layers_dir() -> PathBuf {
    global_hub_dir().join("layers")
}

/// Returns ~/.prompthub/config.yaml
pub fn global_config_path() -> PathBuf {
    global_hub_dir().join("config.yaml")
}

/// Ensure global directories exist
pub fn ensure_dirs() -> anyhow::Result<()> {
    std::fs::create_dir_all(global_layers_dir())?;
    Ok(())
}
