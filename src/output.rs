use crate::error::{PromptHubError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Clipboard,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildOutput {
    pub prompt: String,
    pub params: HashMap<String, String>,
    pub meta: BuildMeta,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildMeta {
    pub layers: Vec<String>,
    pub digest: String,
    pub warnings: Vec<String>,
}

pub fn output_result(
    text: &str,
    format: &OutputFormat,
    params: &HashMap<String, String>,
    layers: &[String],
    warnings: &[String],
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("{}", text);
        }
        OutputFormat::Json => {
            let digest = compute_digest(text, layers);
            let output = BuildOutput {
                prompt: text.to_string(),
                params: params.clone(),
                meta: BuildMeta {
                    layers: layers.to_vec(),
                    digest,
                    warnings: warnings.to_vec(),
                },
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Clipboard => {
            copy_to_clipboard(text)?;
            eprintln!("✓ Prompt copied to clipboard ({} chars)", text.len());
        }
    }
    Ok(())
}

/// Compute a short digest over prompt text and layer names so that two builds
/// with different layer sets (but identical rendered text) produce different digests.
fn compute_digest(text: &str, layers: &[String]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    // Include layer identifiers so the digest is sensitive to the full build config
    for layer in layers {
        hasher.update(b"\x00");
        hasher.update(layer.as_bytes());
    }
    let result = hasher.finalize();
    hex::encode(&result[..8])
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| {
        PromptHubError::Other(format!("Cannot access clipboard: {}", e))
    })?;
    clipboard.set_text(text).map_err(|e| {
        PromptHubError::Other(format!("Cannot copy to clipboard: {}", e))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_digest_deterministic() {
        let text = "Hello, world!";
        let layers = vec!["base/writer:v1.0".to_string()];
        let d1 = compute_digest(text, &layers);
        let d2 = compute_digest(text, &layers);
        assert_eq!(d1, d2, "digest must be deterministic");
    }

    #[test]
    fn test_compute_digest_layer_sensitive() {
        let text = "Same text";
        let d1 = compute_digest(text, &["base/writer:v1.0".to_string()]);
        let d2 = compute_digest(text, &["base/reviewer:v1.0".to_string()]);
        assert_ne!(d1, d2, "different layer sets must produce different digests");
    }

    #[test]
    fn test_compute_digest_is_hex_string() {
        let d = compute_digest("test", &[]);
        assert_eq!(d.len(), 16, "digest should be 8 bytes = 16 hex chars");
        assert!(d.chars().all(|c| c.is_ascii_hexdigit()), "digest should be hex");
    }
}
