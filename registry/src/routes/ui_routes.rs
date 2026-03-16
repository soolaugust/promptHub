use axum::{extract::State, response::Html, Json};
use crate::{AppState, db::RegistryStats, error::Result};

pub async fn serve_ui() -> Html<&'static str> {
    Html(include_str!("../ui.html"))
}

pub async fn get_stats(State(state): State<AppState>) -> Result<Json<RegistryStats>> {
    let stats = state.db.get_stats()?;
    Ok(Json(stats))
}
