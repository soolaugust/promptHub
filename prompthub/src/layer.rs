use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.full_name(), self.meta.version)
    }
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
        let meta: LayerMeta = serde_yaml::from_str(&yaml_content)
            .map_err(|e| crate::error::PromptHubError::Other(
                format!("Cannot parse {}: {}", yaml_path.display(), e)
            ))?;

        let content = if prompt_path.exists() {
            std::fs::read_to_string(&prompt_path)
                .map_err(|e| crate::error::PromptHubError::Other(
                    format!("Cannot read {}: {}", prompt_path.display(), e)
                ))?
        } else {
            String::new()
        };

        // Validate required fields so callers get a clear error rather than
        // silently operating on a layer with no name or version.
        if meta.name.is_empty() {
            return Err(crate::error::PromptHubError::ValidationError(
                format!("{}: 'name' field is required", yaml_path.display())
            ));
        }
        if meta.version.is_empty() {
            return Err(crate::error::PromptHubError::ValidationError(
                format!("{}: 'version' field is required", yaml_path.display())
            ));
        }

        let (sections, dup_warnings) = parse_sections(&content);
        for w in &dup_warnings {
            eprintln!("Warning ({}): {}", prompt_path.display(), w);
        }

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

/// Parse [section-name] blocks from prompt.md content.
/// Returns `(sections_map, duplicate_warnings)`.  When the same section header
/// appears more than once the last occurrence wins and a warning is recorded.
pub fn parse_sections(content: &str) -> (HashMap<String, String>, Vec<String>) {
    let mut sections = HashMap::new();
    let mut warnings = Vec::new();
    let mut current_section: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if let Some(section_name) = parse_section_header(line) {
            // Save previous section
            if let Some(name) = current_section.take() {
                if sections.contains_key(&name) {
                    warnings.push(format!(
                        "Duplicate section '[{}]' in prompt.md; later definition wins",
                        name
                    ));
                }
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
        if sections.contains_key(&name) {
            warnings.push(format!(
                "Duplicate section '[{}]' in prompt.md; later definition wins",
                name
            ));
        }
        sections.insert(name, current_content.trim().to_string());
    }

    (sections, warnings)
}

/// Maximum allowed length (in bytes) for a section header name.
const MAX_SECTION_NAME_LEN: usize = 64;

/// Check if a line is a section header like [section-name]
fn parse_section_header(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
        let inner = &line[1..line.len()-1];
        // Enforce a reasonable maximum length to reject malformed input
        if inner.len() > MAX_SECTION_NAME_LEN {
            return None;
        }
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
    let order_set: HashSet<&String> = order.iter().collect();
    let mut remaining: Vec<&String> = sections.keys()
        .filter(|k| !order_set.contains(*k))
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
        let (sections, warnings) = parse_sections(content);
        assert_eq!(sections.len(), 3);
        assert!(warnings.is_empty());
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

    #[test]
    fn test_parse_sections_duplicate_warning() {
        let content = "[role]\nFirst definition.\n\n[role]\nSecond definition.\n";
        let (sections, warnings) = parse_sections(content);
        // Last occurrence wins
        assert_eq!(sections["role"], "Second definition.");
        assert_eq!(warnings.len(), 1, "one duplicate warning expected");
        assert!(warnings[0].contains("role"), "warning should mention section name");
    }

    #[test]
    fn test_layer_display() {
        use crate::layer::LayerMeta;
        let meta = LayerMeta {
            name: "code-reviewer".to_string(),
            namespace: "base".to_string(),
            version: "v1.2".to_string(),
            description: String::new(),
            author: String::new(),
            tags: Vec::new(),
            sections: Vec::new(),
            conflicts: Vec::new(),
            requires: Vec::new(),
            models: Vec::new(),
        };
        let layer = Layer {
            meta,
            content: String::new(),
            sections: HashMap::new(),
        };
        let displayed = format!("{}", layer);
        assert_eq!(displayed, "base/code-reviewer (v1.2)",
            "Display should show full_name and version");
    }

    #[test]
    fn test_layer_display_no_namespace() {
        use crate::layer::LayerMeta;
        let meta = LayerMeta {
            name: "my-layer".to_string(),
            namespace: String::new(),
            version: "v2.0".to_string(),
            description: String::new(),
            author: String::new(),
            tags: Vec::new(),
            sections: Vec::new(),
            conflicts: Vec::new(),
            requires: Vec::new(),
            models: Vec::new(),
        };
        let layer = Layer {
            meta,
            content: String::new(),
            sections: HashMap::new(),
        };
        let displayed = format!("{}", layer);
        assert_eq!(displayed, "my-layer (v2.0)",
            "Display without namespace should omit namespace prefix");
    }

    #[test]
    fn test_parse_section_header_max_length() {
        // Names at or below the limit are accepted
        let ok_name = "a".repeat(MAX_SECTION_NAME_LEN);
        let ok_header = format!("[{}]", ok_name);
        assert!(parse_section_header(&ok_header).is_some(),
            "name at max length should be accepted");

        // Names exceeding the limit are rejected
        let long_name = "a".repeat(MAX_SECTION_NAME_LEN + 1);
        let long_header = format!("[{}]", long_name);
        assert_eq!(parse_section_header(&long_header), None,
            "name exceeding max length should be rejected");
    }

    #[test]
    fn test_sections_to_content_remaining_sections_sorted() {
        // Sections not in the order list should be appended at the end, sorted
        // alphabetically, so the output is deterministic.
        let mut sections = HashMap::new();
        sections.insert("role".to_string(), "Role content.".to_string());
        sections.insert("zebra".to_string(), "Zebra content.".to_string());
        sections.insert("alpha".to_string(), "Alpha content.".to_string());

        // order only mentions "role"; "zebra" and "alpha" are remainders
        let order = vec!["role".to_string()];
        let content = sections_to_content(&sections, &order);

        // "role" appears first (in-order section)
        let role_pos = content.find("Role content.").expect("role section missing");
        // remaining sections: alpha before zebra (sorted)
        let alpha_pos = content.find("Alpha content.").expect("alpha section missing");
        let zebra_pos = content.find("Zebra content.").expect("zebra section missing");

        assert!(role_pos < alpha_pos, "in-order section should precede remaining sections");
        assert!(alpha_pos < zebra_pos, "remaining sections should be sorted alphabetically");
    }

    #[test]
    fn test_load_from_dir_missing_name_errors() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        // Write a layer.yaml with an empty name
        let yaml = "name: \"\"\nversion: v1.0\n";
        std::fs::write(tmp.path().join("layer.yaml"), yaml).unwrap();
        std::fs::write(tmp.path().join("prompt.md"), "[role]\nContent.\n").unwrap();

        let result = Layer::load_from_dir(tmp.path());
        assert!(result.is_err(), "empty name should produce a validation error");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("name") || msg.contains("required"),
            "error should mention 'name' or 'required', got: {}", msg);
    }

    #[test]
    fn test_load_from_dir_missing_version_errors() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        // Write a layer.yaml with no version field (serde default is empty string)
        let yaml = "name: my-layer\n";
        std::fs::write(tmp.path().join("layer.yaml"), yaml).unwrap();
        std::fs::write(tmp.path().join("prompt.md"), "[role]\nContent.\n").unwrap();

        let result = Layer::load_from_dir(tmp.path());
        assert!(result.is_err(), "empty version should produce a validation error");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("version") || msg.contains("required"),
            "error should mention 'version' or 'required', got: {}", msg);
    }
}
