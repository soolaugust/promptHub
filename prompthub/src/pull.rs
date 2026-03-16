use crate::error::{PromptHubError, Result};
use crate::config::{Config, Source, global_layers_dir};
use crate::parser::LayerRef;
use std::path::PathBuf;

/// Resolve the effective version string to use when pulling a layer.
///
/// When the caller requests "latest" (or provides an empty string), we
/// currently fall back to "v1.0" because there is no remote index to
/// query for the actual latest version.
///
/// TODO: implement index-based latest-version discovery once the remote
/// registry provides an index endpoint.
pub(crate) fn resolve_pull_version(version: &str) -> String {
    if version == "latest" || version.is_empty() {
        "v1.0".to_string()
    } else {
        version.to_string()
    }
}

/// Build the base URL for downloading a specific layer version.
pub(crate) fn layer_url_base(base_url: &str, source: &str, version: &str) -> String {
    format!("{}/layers/{}/{}", base_url.trim_end_matches('/'), source, version)
}

/// Extract the auth token from a source, if any.
pub(crate) fn auth_token_for(source: &Source) -> Option<&str> {
    source.auth.as_ref().map(|a| a.token.as_str())
}

/// Pull a layer from a remote source
pub fn pull_layer(layer_ref: &LayerRef, config: &Config) -> Result<PathBuf> {
    let source = config.default_source().ok_or_else(|| {
        PromptHubError::Other("No sources configured. Add a source to ~/.prompthub/config.yaml".to_string())
    })?;

    let base_url = source.url.trim_end_matches('/');
    let version = resolve_pull_version(&layer_ref.version);

    let url_base = layer_url_base(base_url, &layer_ref.source, &version);

    // Download layer.yaml
    let yaml_url = format!("{}/layer.yaml", url_base);
    let prompt_url = format!("{}/prompt.md", url_base);

    eprintln!("Pulling {} from {}...", layer_ref.display(), source.name);

    let token = auth_token_for(source);
    let yaml_content = fetch_url_with_auth(&yaml_url, token)?;
    let prompt_content = match fetch_url_with_auth(&prompt_url, token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: could not fetch prompt.md for {} ({}); layer will have no prompt content", layer_ref.display(), e);
            String::new()
        }
    };

    // Save to cache
    let dest_dir = global_layers_dir()
        .join(&layer_ref.source)
        .join(&version);

    std::fs::create_dir_all(&dest_dir).map_err(|e| {
        PromptHubError::Other(format!("Cannot create directory {}: {}", dest_dir.display(), e))
    })?;

    std::fs::write(dest_dir.join("layer.yaml"), &yaml_content).map_err(|e| {
        PromptHubError::Other(format!("Cannot write layer.yaml: {}", e))
    })?;

    if !prompt_content.is_empty() {
        std::fs::write(dest_dir.join("prompt.md"), &prompt_content).map_err(|e| {
            PromptHubError::Other(format!("Cannot write prompt.md: {}", e))
        })?;
    }

    eprintln!("✓ Pulled {} to {}", layer_ref.display(), dest_dir.display());
    Ok(dest_dir)
}

/// Default HTTP request timeout (30 seconds).
const FETCH_TIMEOUT_SECS: u64 = 30;

fn fetch_url_with_auth(url: &str, auth_token: Option<&str>) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(PromptHubError::Network)?;

    let mut request = client.get(url);
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }
    let response = request.send().map_err(PromptHubError::Network)?;

    if !response.status().is_success() {
        return Err(PromptHubError::Other(
            format!("HTTP {} for {}", response.status(), url)
        ));
    }

    response.text().map_err(PromptHubError::Network)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_pull_version_latest_becomes_v1_0() {
        assert_eq!(resolve_pull_version("latest"), "v1.0",
            "'latest' should fall back to v1.0 until index discovery is implemented");
    }

    #[test]
    fn test_resolve_pull_version_empty_becomes_v1_0() {
        assert_eq!(resolve_pull_version(""), "v1.0",
            "empty version should fall back to v1.0");
    }

    #[test]
    fn test_resolve_pull_version_explicit_preserved() {
        assert_eq!(resolve_pull_version("v2.3"), "v2.3",
            "explicit version should be returned as-is");
        assert_eq!(resolve_pull_version("v1.0"), "v1.0",
            "v1.0 should be returned as-is");
    }

    #[test]
    fn test_layer_url_base_construction() {
        let url = layer_url_base("https://example.com/registry", "base/reviewer", "v1.0");
        assert_eq!(url, "https://example.com/registry/layers/base/reviewer/v1.0");
    }

    #[test]
    fn test_layer_url_base_strips_trailing_slash() {
        let url = layer_url_base("https://example.com/registry/", "base/writer", "v2.0");
        assert_eq!(url, "https://example.com/registry/layers/base/writer/v2.0",
            "trailing slash in base URL should be stripped");
    }

    #[test]
    fn test_pull_layer_uses_auth_token_when_present() {
        use crate::config::{Source, SourceAuth};

        let source_with_auth = Source {
            name: "private-reg".to_string(),
            url: "https://registry.example.com".to_string(),
            default: true,
            auth: Some(SourceAuth { token: "phrt_test123".to_string() }),
        };
        let source_no_auth = Source {
            name: "official".to_string(),
            url: "https://raw.githubusercontent.com/prompthub/layers/main".to_string(),
            default: false,
            auth: None,
        };

        // `auth_token_for` should return the token when auth is set
        assert_eq!(auth_token_for(&source_with_auth), Some("phrt_test123"));
        assert_eq!(auth_token_for(&source_no_auth), None);
    }
}
