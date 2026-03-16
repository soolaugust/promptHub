use crate::config::Source;
use crate::layer::validate_bytes;

/// Represents a parsed push target: `namespace/name:version`.
#[derive(Debug, Clone)]
pub struct PushTarget {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl PushTarget {
    /// Parse a push target string like `base/my-expert:v1.0`.
    /// `namespace/name:version` — all three components required.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let (name_part, version) = s.split_once(':')
            .ok_or_else(|| anyhow::anyhow!("push target must include version, e.g. base/my-expert:v1.0"))?;
        let (namespace, name) = name_part.split_once('/')
            .ok_or_else(|| anyhow::anyhow!("push target must include namespace, e.g. base/my-expert:v1.0"))?;
        if namespace.is_empty() || name.is_empty() || version.is_empty() {
            anyhow::bail!("push target must be non-empty: namespace/name:version");
        }
        Ok(Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    /// The source path component used to locate the local layer directory.
    /// e.g. `base/my-expert`
    pub fn source_path(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

/// Result of a push operation.
#[derive(Debug)]
pub enum PushResult {
    /// Layer pushed successfully. Contains the source name.
    Success(String),
    /// Version already exists on the registry.
    AlreadyExists(String),
}

/// Push a layer to the registry.
///
/// # Arguments
/// - `target` — parsed push target (namespace/name:version)
/// - `source` — registry source (provides URL and auth token)
/// - `layers_dir` — local base directory to look for layers (default: `./layers`)
pub fn push_layer(
    target: &PushTarget,
    source: &Source,
    layers_dir: &std::path::Path,
) -> anyhow::Result<PushResult> {
    // Step 1: Require auth token
    let token = source.auth.as_ref()
        .map(|a| a.token.as_str())
        .ok_or_else(|| anyhow::anyhow!(
            "No auth token for source '{}'. Run `ph login {}` first.",
            source.name, source.url
        ))?;

    // Step 2: Locate layer directory
    let layer_dir = layers_dir.join(target.source_path());
    if !layer_dir.exists() {
        anyhow::bail!("layer directory not found: {}", layer_dir.display());
    }

    // Step 3: Locate version directory
    let version_dir = layer_dir.join(&target.version);
    if !version_dir.exists() {
        anyhow::bail!(
            "version {} not found in {}",
            target.version,
            layer_dir.display()
        );
    }

    // Step 4: Load layer files
    let yaml_path = version_dir.join("layer.yaml");
    let md_path = version_dir.join("prompt.md");

    let layer_yaml = std::fs::read(&yaml_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", yaml_path.display(), e))?;
    let prompt_md = std::fs::read(&md_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", md_path.display(), e))?;

    // Step 5: Validate locally before network round-trip
    validate_bytes(&layer_yaml, &prompt_md)
        .map_err(|e| anyhow::anyhow!("layer validation failed: {}", e))?;

    // Step 6: PUT multipart to registry
    let url = format!(
        "{}/layers/{}/{}/{}",
        source.url.trim_end_matches('/'),
        target.namespace,
        target.name,
        target.version,
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let form = reqwest::blocking::multipart::Form::new()
        .part("layer.yaml", reqwest::blocking::multipart::Part::bytes(layer_yaml)
            .file_name("layer.yaml")
            .mime_str("application/octet-stream")?)
        .part("prompt.md", reqwest::blocking::multipart::Part::bytes(prompt_md)
            .file_name("prompt.md")
            .mime_str("text/plain")?);

    let response = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .map_err(|e| anyhow::anyhow!("network error: {}", e))?;

    match response.status().as_u16() {
        201 => Ok(PushResult::Success(source.name.clone())),
        409 => Ok(PushResult::AlreadyExists(source.name.clone())),
        status => {
            let body = response.text().unwrap_or_default();
            anyhow::bail!("registry error {}: {}", status, body)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_target_parse_valid() {
        let t = PushTarget::parse("base/my-expert:v1.0").unwrap();
        assert_eq!(t.namespace, "base");
        assert_eq!(t.name, "my-expert");
        assert_eq!(t.version, "v1.0");
    }

    #[test]
    fn test_push_target_parse_missing_version_fails() {
        assert!(PushTarget::parse("base/my-expert").is_err());
    }

    #[test]
    fn test_push_target_parse_missing_namespace_fails() {
        assert!(PushTarget::parse("my-expert:v1.0").is_err());
    }

    #[test]
    fn test_push_target_parse_empty_components_fails() {
        assert!(PushTarget::parse("/:v1.0").is_err());
        assert!(PushTarget::parse("base/:v1.0").is_err());
    }

    #[test]
    fn test_push_target_source_path() {
        let t = PushTarget::parse("base/expert:v2.0").unwrap();
        assert_eq!(t.source_path(), "base/expert");
    }

    #[test]
    fn test_push_layer_errors_without_auth_token() {
        use crate::config::Source;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let target = PushTarget::parse("base/expert:v1.0").unwrap();
        let source = Source {
            name: "my-reg".to_string(),
            url: "https://registry.example.com".to_string(),
            default: true,
            auth: None,  // no token
        };

        let err = push_layer(&target, &source, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("ph login"), "error must suggest ph login");
    }

    #[test]
    fn test_push_layer_errors_when_layer_dir_missing() {
        use crate::config::{Source, SourceAuth};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let target = PushTarget::parse("base/nonexistent:v1.0").unwrap();
        let source = Source {
            name: "my-reg".to_string(),
            url: "https://registry.example.com".to_string(),
            default: true,
            auth: Some(SourceAuth { token: "phrt_test".to_string() }),
        };

        let err = push_layer(&target, &source, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("layer directory not found"));
    }

    #[test]
    fn test_push_layer_errors_when_version_dir_missing() {
        use crate::config::{Source, SourceAuth};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        // Create layer dir but not version subdir
        std::fs::create_dir_all(tmp.path().join("base/expert")).unwrap();

        let target = PushTarget::parse("base/expert:v2.0").unwrap();
        let source = Source {
            name: "my-reg".to_string(),
            url: "https://registry.example.com".to_string(),
            default: true,
            auth: Some(SourceAuth { token: "phrt_test".to_string() }),
        };

        let err = push_layer(&target, &source, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("version v2.0 not found"));
    }
}
