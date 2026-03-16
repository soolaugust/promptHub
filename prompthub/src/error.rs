use thiserror::Error;

#[derive(Error, Debug)]
pub enum PromptHubError {
    #[error("Layer not found: {0}")]
    LayerNotFound(String),

    #[error("Parse error in Promptfile: {0}")]
    ParseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Conflict detected between layers: {0} conflicts with {1}")]
    ConflictError(String, String),

    // Note: undefined variables in prompt templates are surfaced as warnings
    // (returned as Vec<String> from render_variables), not as hard errors.
    // The UndefinedVariable variant has been intentionally removed.

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PromptHubError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_not_found_display() {
        let e = PromptHubError::LayerNotFound("base/reviewer".to_string());
        let msg = e.to_string();
        assert!(msg.contains("base/reviewer"),
            "LayerNotFound message should include the layer name, got: {}", msg);
    }

    #[test]
    fn test_conflict_error_display() {
        let e = PromptHubError::ConflictError(
            "base/writer".to_string(),
            "base/translator".to_string(),
        );
        let msg = e.to_string();
        assert!(msg.contains("base/writer"),
            "ConflictError message should include first layer, got: {}", msg);
        assert!(msg.contains("base/translator"),
            "ConflictError message should include second layer, got: {}", msg);
    }

    #[test]
    fn test_parse_error_display() {
        let e = PromptHubError::ParseError("unexpected token at line 3".to_string());
        let msg = e.to_string();
        assert!(msg.contains("unexpected token"),
            "ParseError should include the detail string, got: {}", msg);
    }

    #[test]
    fn test_validation_error_display() {
        let e = PromptHubError::ValidationError("'name' field is required".to_string());
        let msg = e.to_string();
        assert!(msg.contains("name"),
            "ValidationError should include the detail string, got: {}", msg);
    }
}
