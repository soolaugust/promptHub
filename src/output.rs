use crate::error::{PromptHubError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Clipboard,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Text
    }
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
            let digest = compute_digest(text);
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

fn compute_digest(text: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
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
