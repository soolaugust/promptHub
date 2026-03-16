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
            Ok(serde_yaml::from_str(&content)?)
        } else {
            Ok(Config::default_config())
        }
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
