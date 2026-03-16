use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Metadata stored in layer.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerMeta {
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// A fully loaded layer (metadata + content)
#[derive(Debug, Clone)]
pub struct Layer {
    pub meta: LayerMeta,
    /// Raw prompt.md content
    pub content: String,
    /// Parsed sections: section_name -> content
    pub sections: HashMap<String, String>,
}

impl Layer {
    /// Load a layer from a directory containing layer.yaml and prompt.md
    pub fn load_from_dir(dir: &Path) -> crate::error::Result<Self> {
        let yaml_path = dir.join("layer.yaml");
        let prompt_path = dir.join("prompt.md");

        let yaml_content = std::fs::read_to_string(&yaml_path)
            .map_err(|e| crate::error::PromptHubError::Other(
                format!("Cannot read {}: {}", yaml_path.display(), e)
            ))?;
        let meta: LayerMeta = serde_yaml::from_str(&yaml_content)?;

        let content = if prompt_path.exists() {
            std::fs::read_to_string(&prompt_path)
                .map_err(|e| crate::error::PromptHubError::Other(
                    format!("Cannot read {}: {}", prompt_path.display(), e)
                ))?
        } else {
            String::new()
        };

        let sections = parse_sections(&content);

        Ok(Layer { meta, content, sections })
    }

    /// Full identifier: namespace/name
    pub fn full_name(&self) -> String {
        if self.meta.namespace.is_empty() {
            self.meta.name.clone()
        } else {
            format!("{}/{}", self.meta.namespace, self.meta.name)
        }
    }
}

/// Parse [section-name] blocks from prompt.md content
pub fn parse_sections(content: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current_section: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if let Some(section_name) = parse_section_header(line) {
            // Save previous section
            if let Some(name) = current_section.take() {
                sections.insert(name, current_content.trim().to_string());
                current_content.clear();
            }
            current_section = Some(section_name);
        } else if current_section.is_some() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Save the last section
    if let Some(name) = current_section {
        sections.insert(name, current_content.trim().to_string());
    }

    sections
}

/// Check if a line is a section header like [section-name]
fn parse_section_header(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
        let inner = &line[1..line.len()-1];
        // Make sure it's a valid identifier (alphanumeric, hyphens, underscores)
        if inner.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Some(inner.to_lowercase());
        }
    }
    None
}

/// Reassemble sections back to markdown, maintaining a stable section order
pub fn sections_to_content(sections: &HashMap<String, String>, order: &[String]) -> String {
    let mut result = String::new();

    // First write sections in the given order
    for section_name in order {
        if let Some(content) = sections.get(section_name) {
            result.push_str(&format!("[{}]\n{}\n\n", section_name, content));
        }
    }

    // Then write any remaining sections not in the order list
    let mut remaining: Vec<&String> = sections.keys()
        .filter(|k| !order.contains(k))
        .collect();
    remaining.sort();
    for section_name in remaining {
        if let Some(content) = sections.get(section_name) {
            result.push_str(&format!("[{}]\n{}\n\n", section_name, content));
        }
    }

    result.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sections_basic() {
        let content = r#"[role]
You are an expert.

[constraints]
- Be concise
- Be accurate

[output-format]
Use markdown."#;
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 3);
        assert!(sections.contains_key("role"));
        assert!(sections.contains_key("constraints"));
        assert!(sections.contains_key("output-format"));
        assert!(sections["role"].contains("You are an expert"));
        assert!(sections["constraints"].contains("Be concise"));
    }

    #[test]
    fn test_parse_section_header() {
        assert_eq!(parse_section_header("[role]"), Some("role".to_string()));
        assert_eq!(parse_section_header("[output-format]"), Some("output-format".to_string()));
        assert_eq!(parse_section_header("  [role]  "), Some("role".to_string()));
        assert_eq!(parse_section_header("not a header"), None);
        assert_eq!(parse_section_header("[]"), None);
        assert_eq!(parse_section_header("[role"), None);
    }

    #[test]
    fn test_sections_to_content() {
        let mut sections = HashMap::new();
        sections.insert("role".to_string(), "You are an expert.".to_string());
        sections.insert("constraints".to_string(), "- Be concise".to_string());

        let order = vec!["role".to_string(), "constraints".to_string()];
        let content = sections_to_content(&sections, &order);
        assert!(content.contains("[role]"));
        assert!(content.contains("[constraints]"));
        let role_pos = content.find("[role]").unwrap();
        let constraints_pos = content.find("[constraints]").unwrap();
        assert!(role_pos < constraints_pos);
    }
}
