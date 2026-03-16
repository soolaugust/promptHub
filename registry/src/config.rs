use serde::Deserialize;
use anyhow::Context;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub read_only: bool,
}

fn default_port() -> u16 { 8080 }

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    S3 {
        endpoint: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        #[serde(default = "default_region")]
        region: String,
    },
    Filesystem {
        path: String,
    },
}

fn default_region() -> String { "us-east-1".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub pull_requires_auth: bool,
    pub admin_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: default_log_level() }
    }
}

fn default_log_level() -> String { "info".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub log: LogConfig,
}

impl RegistryConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config file: {}", path))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("cannot parse config file: {}", path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filesystem_config() {
        let yaml = r#"
server:
  port: 9090
  read_only: false
storage:
  type: filesystem
  path: /tmp/layers
database:
  path: /tmp/registry.db
auth:
  pull_requires_auth: false
  admin_token: "phrt_test"
"#;
        let cfg: RegistryConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.server.port, 9090);
        assert!(!cfg.server.read_only);
        match &cfg.storage {
            StorageConfig::Filesystem { path } => assert_eq!(path, "/tmp/layers"),
            _ => panic!("expected filesystem storage"),
        }
        assert_eq!(cfg.auth.admin_token, "phrt_test");
    }

    #[test]
    fn test_parse_s3_config() {
        let yaml = r#"
server:
  port: 8080
storage:
  type: s3
  endpoint: http://minio:9000
  bucket: prompthub
  access_key: admin
  secret_key: secret
database:
  path: /data/registry.db
auth:
  pull_requires_auth: true
  admin_token: "phrt_admin"
"#;
        let cfg: RegistryConfig = serde_yaml::from_str(yaml).unwrap();
        match &cfg.storage {
            StorageConfig::S3 { endpoint, bucket, .. } => {
                assert_eq!(endpoint, "http://minio:9000");
                assert_eq!(bucket, "prompthub");
            }
            _ => panic!("expected s3 storage"),
        }
        assert!(cfg.auth.pull_requires_auth);
    }
}
