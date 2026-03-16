use crate::error::{PromptHubError, Result};
use crate::config::{Config, global_layers_dir};
use crate::parser::LayerRef;
use std::path::PathBuf;

/// Pull a layer from a remote source
pub fn pull_layer(layer_ref: &LayerRef, config: &Config) -> Result<PathBuf> {
    let source = config.default_source().ok_or_else(|| {
        PromptHubError::Other("No sources configured. Add a source to ~/.prompthub/config.yaml".to_string())
    })?;

    let base_url = source.url.trim_end_matches('/');
    let version = if layer_ref.version == "latest" || layer_ref.version.is_empty() {
        // Try to find latest from index or just use "v1.0" as default
        "v1.0".to_string()
    } else {
        layer_ref.version.clone()
    };

    let layer_url_base = format!("{}/layers/{}/{}", base_url, layer_ref.source, version);

    // Download layer.yaml
    let yaml_url = format!("{}/layer.yaml", layer_url_base);
    let prompt_url = format!("{}/prompt.md", layer_url_base);

    eprintln!("Pulling {} from {}...", layer_ref.display(), source.name);

    let yaml_content = fetch_url(&yaml_url)?;
    let prompt_content = fetch_url(&prompt_url).unwrap_or_default();

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

fn fetch_url(url: &str) -> Result<String> {
    let response = reqwest::blocking::get(url).map_err(|e| {
        PromptHubError::Network(e)
    })?;

    if !response.status().is_success() {
        return Err(PromptHubError::Other(
            format!("HTTP {} for {}", response.status(), url)
        ));
    }

    response.text().map_err(|e| PromptHubError::Network(e))
}
