mod auth;
mod config;
mod db;
mod error;
mod routes;
mod storage;

use std::sync::Arc;
use config::RegistryConfig;
use db::Db;
use storage::StorageBackend;

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
    let _config = RegistryConfig::load(&config_path)?;
    println!("ph-registry starting (stub)");
    Ok(())
}
