use crate::error::{PromptHubError, Result};
use crate::layer::Layer;
use std::collections::{HashMap, HashSet};

/// Result of merging multiple layers
#[derive(Debug, Clone)]
pub struct MergedPrompt {
    /// Final merged sections (section_name -> content)
    pub sections: HashMap<String, String>,
    /// Order of sections for output
    pub section_order: Vec<String>,
    /// Warnings generated during merge
    pub warnings: Vec<String>,
    /// Params from Promptfile
    pub params: HashMap<String, String>,
}

impl MergedPrompt {
    /// Render merged sections to a single string
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        let mut written = std::collections::HashSet::new();

        // Write in defined order
        for name in &self.section_order {
            if let Some(content) = self.sections.get(name) {
                if !content.is_empty() {
                    parts.push(content.clone());
                    written.insert(name.clone());
                }
            }
        }

        // Write remaining sections (not in order list)
        let mut remaining: Vec<&String> = self.sections.keys()
            .filter(|k| !written.contains(*k))
            .collect();
        remaining.sort();
        for name in remaining {
            if let Some(content) = self.sections.get(name) {
                if !content.is_empty() {
                    parts.push(content.clone());
                }
            }
        }

        parts.join("\n\n")
    }
}

/// Merge a base layer with additional layers
pub fn merge_layers(
    base: &Layer,
    additional: &[Layer],
    params: HashMap<String, String>,
) -> Result<MergedPrompt> {
    let mut warnings = Vec::new();

    // Check conflicts first
    check_conflicts(base, additional)?;

    // Start with base layer sections
    let mut merged_sections = base.sections.clone();
    let mut section_order: Vec<String> = base.meta.sections.clone();
    // Track which names are already in section_order for O(1) dedup
    let mut order_set: HashSet<String> = section_order.iter().cloned().collect();

    // Add sections present in base.sections but not yet in order
    for key in base.sections.keys() {
        if order_set.insert(key.clone()) {
            section_order.push(key.clone());
        }
    }

    // Apply each additional layer
    for layer in additional {
        for (section_name, content) in &layer.sections {
            if merged_sections.contains_key(section_name) {
                // Same section name → later layer overrides
                warnings.push(format!(
                    "Section '{}' overridden by layer '{}'",
                    section_name,
                    layer.full_name()
                ));
                merged_sections.insert(section_name.clone(), content.clone());
            } else {
                // New section → append
                merged_sections.insert(section_name.clone(), content.clone());
                if order_set.insert(section_name.clone()) {
                    section_order.push(section_name.clone());
                }
            }
        }
    }

    Ok(MergedPrompt {
        sections: merged_sections,
        section_order,
        warnings,
        params,
    })
}

/// Check for conflicts between layers
fn check_conflicts(base: &Layer, additional: &[Layer]) -> Result<()> {
    let all_layers: Vec<&Layer> = std::iter::once(base).chain(additional.iter()).collect();

    // Build a set of all present layer identifiers (both full_name and short name)
    // so conflict lookups are O(1) instead of O(n).
    // Value is index into all_layers for error reporting.
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for (idx, layer) in all_layers.iter().enumerate() {
        name_to_idx.entry(layer.full_name()).or_insert(idx);
        name_to_idx.entry(layer.meta.name.clone()).or_insert(idx);
    }

    for (i, layer) in all_layers.iter().enumerate() {
        for conflict in &layer.meta.conflicts {
            if let Some(&j) = name_to_idx.get(conflict.as_str()) {
                if i != j {
                    return Err(PromptHubError::ConflictError(
                        layer.full_name(),
                        all_layers[j].full_name(),
                    ));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerMeta;

    fn make_layer(name: &str, namespace: &str, sections: Vec<(&str, &str)>, conflicts: Vec<&str>) -> Layer {
        let mut section_map = HashMap::new();
        let section_names: Vec<String> = sections.iter().map(|(n, _)| n.to_string()).collect();
        for (name, content) in sections {
            section_map.insert(name.to_string(), content.to_string());
        }
        Layer {
            meta: LayerMeta {
                name: name.to_string(),
                namespace: namespace.to_string(),
                version: "v1.0".to_string(),
                description: String::new(),
                author: String::new(),
                tags: Vec::new(),
                sections: section_names,
                conflicts: conflicts.iter().map(|s| s.to_string()).collect(),
                requires: Vec::new(),
                models: Vec::new(),
            },
            content: String::new(),
            sections: section_map,
        }
    }

    #[test]
    fn test_merge_new_sections_appended() {
        let base = make_layer("reviewer", "base", vec![
            ("role", "You are a reviewer."),
        ], vec![]);
        let extra = make_layer("concise", "style", vec![
            ("constraints", "Be concise."),
        ], vec![]);

        let result = merge_layers(&base, &[extra], HashMap::new()).unwrap();
        assert!(result.sections.contains_key("role"));
        assert!(result.sections.contains_key("constraints"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_merge_same_section_overridden() {
        let base = make_layer("reviewer", "base", vec![
            ("role", "You are a reviewer."),
            ("constraints", "Original constraints."),
        ], vec![]);
        let extra = make_layer("override", "style", vec![
            ("constraints", "New constraints."),
        ], vec![]);

        let result = merge_layers(&base, &[extra], HashMap::new()).unwrap();
        assert_eq!(result.sections["constraints"], "New constraints.");
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_conflict_detection() {
        let base = make_layer("writer", "base", vec![
            ("role", "You are a writer."),
        ], vec!["base/translator"]);
        let extra = make_layer("translator", "base", vec![
            ("role", "You are a translator."),
        ], vec![]);

        let result = merge_layers(&base, &[extra], HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_section_order_maintained() {
        let base = make_layer("reviewer", "base", vec![
            ("role", "Role content."),
            ("constraints", "Constraints content."),
            ("output-format", "Output format."),
        ], vec![]);

        let result = merge_layers(&base, &[], HashMap::new()).unwrap();
        let text = result.to_text();
        let role_pos = text.find("Role content.").unwrap();
        let constraints_pos = text.find("Constraints content.").unwrap();
        let output_pos = text.find("Output format.").unwrap();
        assert!(role_pos < constraints_pos);
        assert!(constraints_pos < output_pos);
    }

    #[test]
    fn test_sections_not_in_meta_sections_are_included() {
        // A layer may have sections in prompt.md that are NOT listed in meta.sections.
        // They should still appear in the merged output (not silently dropped).
        use crate::layer::LayerMeta;
        let mut section_map = HashMap::new();
        section_map.insert("role".to_string(), "You are helpful.".to_string());
        section_map.insert("extra".to_string(), "Bonus content.".to_string());

        let base = Layer {
            meta: LayerMeta {
                name: "test".to_string(),
                namespace: "base".to_string(),
                version: "v1.0".to_string(),
                description: String::new(),
                author: String::new(),
                tags: Vec::new(),
                // meta.sections only lists "role", not "extra"
                sections: vec!["role".to_string()],
                conflicts: Vec::new(),
                requires: Vec::new(),
                models: Vec::new(),
            },
            content: String::new(),
            sections: section_map,
        };

        let result = merge_layers(&base, &[], HashMap::new()).unwrap();
        let text = result.to_text();
        assert!(text.contains("You are helpful."), "role section missing from output");
        assert!(text.contains("Bonus content."), "undeclared 'extra' section should be in output");
    }
}
