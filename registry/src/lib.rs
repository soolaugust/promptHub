pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod routes;
pub mod storage;

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
