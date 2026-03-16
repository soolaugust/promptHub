mod auth;
mod config;
mod db;
mod error;
mod routes;
mod storage;

use std::sync::Arc;
use axum::{Router, routing::{get, post, put}};
use config::RegistryConfig;
use db::Db;
use storage::{FilesystemStorage, StorageBackend};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RegistryConfig>,
    pub db: Db,
    pub storage: Arc<StorageBackend>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = std::env::args().nth(1)
        .unwrap_or_else(|| "registry.yaml".to_string());
    let config = RegistryConfig::load(&config_path)?;

    let level = config.log.level.clone();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(level))
        .init();

    let db = Db::open(&config.database.path)?;

    let storage: Arc<StorageBackend> = match &config.storage {
        config::StorageConfig::Filesystem { path } => {
            std::fs::create_dir_all(path)?;
            Arc::new(StorageBackend::Filesystem(FilesystemStorage::new(path)))
        }
        config::StorageConfig::S3 { .. } => {
            anyhow::bail!("S3 storage not yet implemented; use type: filesystem");
        }
    };

    let port = config.server.port;
    let state = AppState {
        config: Arc::new(config),
        db,
        storage,
    };

    let app = Router::new()
        .route("/v1/auth/login", post(routes::auth_routes::login))
        .route("/v1/auth/token", post(routes::auth_routes::issue_token))
        .route("/layers/:namespace/:name/versions", get(routes::layer_routes::get_versions))
        .route("/layers", get(routes::layer_routes::list_layers))
        .route("/layers/:namespace/:name/:version/:filename",
               get(routes::layer_routes::get_layer_file))
        .route("/layers/:namespace/:name/:version",
               put(routes::layer_routes::put_layer))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("ph-registry listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
