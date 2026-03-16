use crate::error::{PromptHubError, Result};
use crate::merger::MergedPrompt;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use regex::Regex;

static VAR_REGEX: OnceLock<Regex> = OnceLock::new();

fn var_regex() -> &'static Regex {
    VAR_REGEX.get_or_init(|| Regex::new(r"\$\{([^}]+)\}").expect("invalid regex"))
}

/// Render variables in merged prompt content.
/// Returns `(rendered_text, undefined_var_warnings)`.
pub fn render_variables(
    merged: &MergedPrompt,
    vars: &HashMap<String, String>,
    task: Option<&str>,
    includes: &[(std::path::PathBuf, String)],
) -> Result<(String, Vec<String>)> {
    let mut text = merged.to_text();
    let mut undef_warnings: Vec<String> = Vec::new();

    // Process INCLUDE content (append each included file)
    for (_path, content) in includes {
        text.push_str("\n\n");
        text.push_str(content);
    }

    // Substitute ${var_name} placeholders
    let (substituted, body_warns) = substitute_vars(&text, vars)?;
    text = substituted;
    undef_warnings.extend(body_warns);

    // Append TASK at the end
    if let Some(task_text) = task {
        let (rendered_task, task_warns) = substitute_vars(task_text, vars)?;
        undef_warnings.extend(task_warns);
        text.push_str("\n\n---\n\n");
        text.push_str(&rendered_task);
    }

    Ok((text, undef_warnings))
}

/// Substitute ${var_name} in text with values from vars map.
/// Returns `(substituted_text, list_of_undefined_variable_names)`.
pub fn substitute_vars(text: &str, vars: &HashMap<String, String>) -> Result<(String, Vec<String>)> {
    let re = var_regex();
    let mut undef = Vec::new();
    let result = re.replace_all(text, |caps: &regex::Captures| {
        let var_name = &caps[1];
        if let Some(value) = vars.get(var_name) {
            value.clone()
        } else {
            undef.push(var_name.to_string());
            caps[0].to_string() // Keep original placeholder if not found
        }
    }).to_string();

    Ok((result, undef))
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
        let (result, undef) = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "Review in 中文 by Alice");
        assert!(undef.is_empty());
    }

    #[test]
    fn test_substitute_undefined_var_kept() {
        let vars = HashMap::new();
        let text = "Review in ${language}";
        let (result, undef) = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "Review in ${language}");
        assert_eq!(undef, vec!["language".to_string()],
            "undefined variable should be reported");
    }

    #[test]
    fn test_substitute_multiple_occurrences() {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), "42".to_string());
        let text = "${x} + ${x} = 84";
        let (result, undef) = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "42 + 42 = 84");
        assert!(undef.is_empty());
    }

    #[test]
    fn test_substitute_undefined_var_reported_once_per_occurrence() {
        let vars = HashMap::new();
        // Same undefined var used twice - both occurrences are reported
        let text = "${missing} and ${missing}";
        let (result, undef) = substitute_vars(text, &vars).unwrap();
        assert_eq!(result, "${missing} and ${missing}");
        assert_eq!(undef.len(), 2, "each occurrence is reported separately");
    }

    #[test]
    fn test_render_variables_with_includes_and_task() {
        use crate::merger::MergedPrompt;
        use std::collections::HashMap;
        use std::path::PathBuf;

        let mut sections = HashMap::new();
        sections.insert("role".to_string(), "You are a ${role}.".to_string());
        let merged = MergedPrompt {
            sections,
            section_order: vec!["role".to_string()],
            params: HashMap::new(),
            warnings: Vec::new(),
        };

        let mut vars = HashMap::new();
        vars.insert("role".to_string(), "code reviewer".to_string());

        let include_path = PathBuf::from("context.md");
        let includes = vec![(include_path, "Extra context here.".to_string())];

        let task = Some("Review this code.");

        let (text, undef) = render_variables(&merged, &vars, task, &includes).unwrap();

        assert!(text.contains("You are a code reviewer."),
            "variable substitution should work in body");
        assert!(text.contains("Extra context here."),
            "include content should be appended");
        assert!(text.contains("---"), "task separator should be present");
        assert!(text.contains("Review this code."),
            "task text should be appended");
        assert!(undef.is_empty(), "no undefined vars expected");
    }

    #[test]
    fn test_render_variables_task_with_undef_var() {
        use crate::merger::MergedPrompt;
        use std::collections::HashMap;

        let mut sections = HashMap::new();
        sections.insert("body".to_string(), "Body text.".to_string());
        let merged = MergedPrompt {
            sections,
            section_order: vec!["body".to_string()],
            params: HashMap::new(),
            warnings: Vec::new(),
        };

        let vars = HashMap::new();
        let task = Some("Review in ${language}.");

        let (text, undef) = render_variables(&merged, &vars, task, &[]).unwrap();

        assert!(text.contains("${language}"), "undefined var kept in task");
        assert_eq!(undef, vec!["language".to_string()],
            "undefined var in task should be reported");
    }
}
