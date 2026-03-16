use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use crate::{AppState, auth, error::{RegistryError, Result}};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: Option<String>,
}

#[derive(Deserialize)]
pub struct IssueTokenRequest {
    pub name: String,
    pub expires_in_days: Option<i64>,
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>> {
    let user = state.db.get_user_by_username(&req.username)?
        .ok_or(RegistryError::Unauthorized)?;
    let (user_id, hash) = user;
    if !auth::verify_password(&req.password, &hash) {
        return Err(RegistryError::Unauthorized);
    }
    let token = auth::generate_token();
    let expires_at = None::<String>; // default: no expiry
    state.db.insert_token(&token, Some(user_id), None, expires_at.as_deref())?;
    Ok(Json(TokenResponse { token, expires_at }))
}

pub async fn issue_token(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<IssueTokenRequest>,
) -> Result<impl axum::response::IntoResponse> {
    // Only admin_token can issue long-lived tokens
    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let bearer = auth::extract_bearer(auth_header)
        .ok_or(RegistryError::Unauthorized)?;
    if bearer != state.config.auth.admin_token {
        return Err(RegistryError::Unauthorized);
    }

    let expires_at = req.expires_in_days.map(|days| {
        (chrono::Utc::now() + chrono::Duration::days(days)).to_rfc3339()
    });
    let token = auth::generate_token();
    state.db.insert_token(&token, None, Some(&req.name), expires_at.as_deref())?;
    // Spec requires 201 Created for token issuance
    Ok((axum::http::StatusCode::CREATED, Json(TokenResponse { token, expires_at })))
}
