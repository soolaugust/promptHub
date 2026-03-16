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
