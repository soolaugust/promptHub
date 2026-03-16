use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Context;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub default: bool,
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
                }
            ],
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = global_config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn default_source(&self) -> Option<&Source> {
        self.sources.iter().find(|s| s.default).or_else(|| self.sources.first())
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
                Source { name: "mirror".to_string(), url: "https://mirror.example.com".to_string(), default: false },
                Source { name: "secondary".to_string(), url: "https://secondary.example.com".to_string(), default: false },
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
                },
                Source {
                    name: "mirror".to_string(),
                    url: "https://mirror.example.com/layers".to_string(),
                    default: false,
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
