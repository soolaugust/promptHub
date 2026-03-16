use crate::error::{RegistryError, Result};
use std::future::Future;
use std::path::PathBuf;

/// Abstraction over where layer files are stored.
///
/// Uses Rust 1.75+ RPITIT (`impl Future`) — no `async-trait` crate needed.
/// Note: RPITIT traits are NOT object-safe (`Box<dyn Storage>` / `Arc<dyn Storage>` are
/// rejected by the compiler). `AppState` uses `Arc<StorageBackend>` where `StorageBackend`
/// is a concrete enum that dispatches to the correct backend. This avoids dynamic dispatch
/// while keeping the trait clean for test use with `FilesystemStorage` directly.
pub trait Storage: Send + Sync {
    fn put(&self, key: &str, data: Vec<u8>) -> impl Future<Output = Result<()>> + Send;
    fn get(&self, key: &str) -> impl Future<Output = Result<Vec<u8>>> + Send;
    fn exists(&self, key: &str) -> impl Future<Output = Result<bool>> + Send;
}

/// Concrete enum that dispatches to the chosen storage backend.
/// Used in `AppState` to avoid `dyn Storage` (RPITIT is not object-safe).
pub enum StorageBackend {
    Filesystem(FilesystemStorage),
    // S3(S3Storage) will be added in a future iteration
}

impl StorageBackend {
    pub async fn put(&self, key: &str, data: Vec<u8>) -> Result<()> {
        match self {
            StorageBackend::Filesystem(s) => s.put(key, data).await,
        }
    }
    pub async fn get(&self, key: &str) -> Result<Vec<u8>> {
        match self {
            StorageBackend::Filesystem(s) => s.get(key).await,
        }
    }
    pub async fn exists(&self, key: &str) -> Result<bool> {
        match self {
            StorageBackend::Filesystem(s) => s.exists(key).await,
        }
    }
}

/// Local filesystem storage backend (development / small teams).
pub struct FilesystemStorage {
    base: PathBuf,
}

impl FilesystemStorage {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl Storage for FilesystemStorage {
    async fn put(&self, key: &str, data: Vec<u8>) -> Result<()> {
        let path = self.base.join(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| RegistryError::Storage(e.to_string()))?;
        }
        tokio::fs::write(&path, data).await
            .map_err(|e| RegistryError::Storage(e.to_string()))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.base.join(key);
        tokio::fs::read(&path).await
            .map_err(|_| RegistryError::NotFound(key.to_string()))
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        // Use tokio::fs::try_exists — avoids blocking the async executor
        tokio::fs::try_exists(self.base.join(key)).await
            .map_err(|e| RegistryError::Storage(e.to_string()))
    }
}

/// Build the S3-style object key for a layer file.
/// e.g. layer_key("base", "expert", "v1.0", "layer.yaml")
///    → "layers/base/expert/v1.0/layer.yaml"
pub fn layer_key(namespace: &str, name: &str, version: &str, filename: &str) -> String {
    format!("layers/{}/{}/{}/{}", namespace, name, version, filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_filesystem_put_and_get() {
        let dir = TempDir::new().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        storage.put("layers/base/expert/v1.0/layer.yaml", b"name: expert".to_vec()).await.unwrap();
        let data = storage.get("layers/base/expert/v1.0/layer.yaml").await.unwrap();
        assert_eq!(data, b"name: expert");
    }

    #[tokio::test]
    async fn test_filesystem_get_missing_returns_not_found() {
        let dir = TempDir::new().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        let result = storage.get("nonexistent/key").await;
        assert!(matches!(result, Err(RegistryError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_filesystem_exists() {
        let dir = TempDir::new().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        assert!(!storage.exists("foo/bar").await.unwrap());
        storage.put("foo/bar", b"data".to_vec()).await.unwrap();
        assert!(storage.exists("foo/bar").await.unwrap());
    }

    #[test]
    fn test_layer_key_format() {
        assert_eq!(
            layer_key("base", "expert", "v1.0", "layer.yaml"),
            "layers/base/expert/v1.0/layer.yaml"
        );
    }
}
