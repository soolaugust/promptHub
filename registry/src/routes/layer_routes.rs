use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::{AppState, auth, error::{RegistryError, Result}};
use crate::storage::layer_key;

/// Require auth only when pull_requires_auth is true.
fn check_pull_auth(state: &AppState, headers: &HeaderMap) -> Result<()> {
    if !state.config.auth.pull_requires_auth {
        return Ok(());
    }
    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    auth::require_auth(&state.db, auth_header, &state.config.auth.admin_token)?;
    Ok(())
}

pub async fn get_layer_file(
    State(state): State<AppState>,
    Path((namespace, name, version, filename)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    check_pull_auth(&state, &headers)?;

    if filename != "layer.yaml" && filename != "prompt.md" {
        return Err(RegistryError::NotFound(filename));
    }
    let key = layer_key(&namespace, &name, &version, &filename);
    let data = state.storage.get(&key).await?;
    Ok(data)
}

pub async fn put_layer(
    State(state): State<AppState>,
    Path((namespace, name, version)): Path<(String, String, String)>,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse> {
    // Always require auth for push
    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    auth::require_auth(&state.db, auth_header, &state.config.auth.admin_token)?;

    if state.config.server.read_only {
        return Err(RegistryError::BadRequest("registry is read-only".to_string()));
    }

    // Parse multipart fields
    let mut layer_yaml: Option<Vec<u8>> = None;
    let mut prompt_md: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await
        .map_err(|e| RegistryError::BadRequest(e.to_string()))? {
        match field.name() {
            Some("layer.yaml") => {
                layer_yaml = Some(field.bytes().await
                    .map_err(|e| RegistryError::BadRequest(e.to_string()))?.to_vec());
            }
            Some("prompt.md") => {
                prompt_md = Some(field.bytes().await
                    .map_err(|e| RegistryError::BadRequest(e.to_string()))?.to_vec());
            }
            _ => {}
        }
    }

    let layer_yaml = layer_yaml
        .ok_or_else(|| RegistryError::BadRequest("missing layer.yaml field".to_string()))?;
    let prompt_md = prompt_md
        .ok_or_else(|| RegistryError::BadRequest("missing prompt.md field".to_string()))?;

    // Validate layer content
    prompthub::layer::validate_bytes(&layer_yaml, &prompt_md)
        .map_err(RegistryError::BadRequest)?;

    // Check version immutability
    if state.db.layer_exists(&namespace, &name, &version)? {
        return Err(RegistryError::Conflict(
            format!("{}/{}/{}", namespace, name, version)
        ));
    }

    // Write to storage (S3-first ordering — metadata inserted only after both writes succeed)
    state.storage.put(&layer_key(&namespace, &name, &version, "layer.yaml"), layer_yaml.clone()).await?;
    state.storage.put(&layer_key(&namespace, &name, &version, "prompt.md"), prompt_md).await?;

    // Parse meta for description/tags — re-parse layer_yaml only (already validated)
    // We can't call validate_bytes again with empty prompt_md, so parse directly from yaml
    let meta: Option<prompthub::layer::LayerMeta> = serde_yaml::from_slice(&layer_yaml).ok();
    let description = meta.as_ref().and_then(|m| {
        if m.description.is_empty() { None } else { Some(m.description.as_str()) }
    });
    let tags = meta.as_ref().map(|m| m.tags.clone()).unwrap_or_default();

    state.db.insert_layer(&namespace, &name, &version, description, &tags, None)?;

    Ok((axum::http::StatusCode::CREATED, Json(serde_json::json!({
        "namespace": namespace,
        "name": name,
        "version": version,
    }))))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

pub async fn list_layers(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::db::LayerSummary>>> {
    check_pull_auth(&state, &headers)?;
    let layers = if let Some(q) = params.q.filter(|s| !s.is_empty()) {
        state.db.search_layers(&q)?
    } else {
        state.db.list_layers()?
    };
    Ok(Json(layers))
}

pub async fn get_versions(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    check_pull_auth(&state, &headers)?;
    let versions = state.db.get_versions(&namespace, &name)?;
    Ok(Json(serde_json::json!({
        "namespace": namespace,
        "name": name,
        "versions": versions,
    })))
}
