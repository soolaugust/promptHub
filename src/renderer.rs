use crate::error::{PromptHubError, Result};
use crate::merger::MergedPrompt;
use std::collections::HashMap;
use std::path::Path;
use regex::Regex;

/// Render variables in merged prompt content
pub fn render_variables(
    merged: &MergedPrompt,
    vars: &HashMap<String, String>,
    task: Option<&str>,
    includes: &[(std::path::PathBuf, String)],
    _base_dir: &Path,
) -> Result<String> {
    let mut text = merged.to_text();

    // Process INCLUDE content (append each included file)
    for (_path, content) in includes {
        text.push_str("\n\n");
        text.push_str(content);
    }

    // Substitute ${var_name} placeholders
    text = substitute_vars(&text, vars)?;

    // Append TASK at the end
    if let Some(task_text) = task {
        let rendered_task = substitute_vars(task_text, vars)?;
        text.push_str("\n\n---\n\n");
        text.push_str(&rendered_task);
    }

    Ok(text)
}

/// Substitute ${var_name} in text with values from vars map
pub fn substitute_vars(text: &str, vars: &HashMap<String, String>) -> Result<String> {
    let re = Regex::new(r"\$\{([^}]+)\}").unwrap();
    let mut warnings = Vec::new();
    let result = re.replace_all(text, |caps: &regex::Captures| {
        let var_name = &caps[1];
        if let Some(value) = vars.get(var_name) {
            value.clone()
        } else {
            warnings.push(var_name.to_string());
            caps[0].to_string() // Keep original if not found
        }
    }).to_string();

    if !warnings.is_empty() {
        eprintln!("Warning: undefined variables: {}", warnings.join(", "));
    }

    Ok(result)
}

/// Load INCLUDE file content
pub fn load_include(path: &std::path::Path, base_dir: &Path) -> Result<String> {
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };

    std::fs::read_to_string(&full_path).map_err(|e| {
        PromptHubError::Other(format!("Cannot read include file '{}': {}", full_path.display(), e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_vars() {
        let mut vars = HashMap::new();
        vars.insert("language".to_string(), "中文".to_string());
        vars.insert("name".to_string(), "Alice".to_string());

        let text = "Review in ${language} by ${name}";
        let result = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "Review in 中文 by Alice");
    }

    #[test]
    fn test_substitute_undefined_var_kept() {
        let vars = HashMap::new();
        let text = "Review in ${language}";
        let result = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "Review in ${language}");
    }

    #[test]
    fn test_substitute_multiple_occurrences() {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), "42".to_string());
        let text = "${x} + ${x} = 84";
        let result = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "42 + 42 = 84");
    }
}
