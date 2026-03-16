use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("unauthorized")]
    Unauthorized,

    #[error("layer not found: {0}")]
    NotFound(String),

    #[error("version already exists: {0}")]
    Conflict(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for RegistryError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            RegistryError::Unauthorized     => (StatusCode::UNAUTHORIZED, self.to_string()),
            RegistryError::NotFound(_)      => (StatusCode::NOT_FOUND, self.to_string()),
            RegistryError::Conflict(_)      => (StatusCode::CONFLICT, self.to_string()),
            RegistryError::BadRequest(_)    => (StatusCode::BAD_REQUEST, self.to_string()),
            RegistryError::Database(e)      => {
                tracing::error!("database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
            RegistryError::Storage(msg)     => {
                tracing::error!("storage error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "storage error".to_string())
            }
            RegistryError::Internal(msg)    => {
                tracing::error!("internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({"error": message}))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, RegistryError>;
