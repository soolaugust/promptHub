// registry/tests/integration.rs
// End-to-end test: spin up a real Axum router against in-memory DB
// and filesystem storage, then exercise push/pull/list/auth flows.

use std::sync::Arc;
use axum::{Router, routing::{get, post, put}};
use axum_test::TestServer;
use ph_registry::{
    AppState,
    auth,
    config::{RegistryConfig, ServerConfig, StorageConfig, DatabaseConfig, AuthConfig, LogConfig},
    db::Db,
    storage::{FilesystemStorage, StorageBackend},
};
use tempfile::TempDir;

fn build_test_app(tmp: &TempDir) -> TestServer {
    let config = Arc::new(RegistryConfig {
        server: ServerConfig { port: 0, read_only: false },
        storage: StorageConfig::Filesystem {
            path: tmp.path().join("layers").to_str().unwrap().to_string(),
        },
        database: DatabaseConfig { path: ":memory:".to_string() },
        auth: AuthConfig {
            pull_requires_auth: false,
            admin_token: "phrt_admin".to_string(),
        },
        log: LogConfig::default(),
    });

    let db = Db::open(":memory:").unwrap();
    // Storage uses StorageBackend enum — not Arc<FilesystemStorage> directly
    let storage = Arc::new(StorageBackend::Filesystem(
        FilesystemStorage::new(tmp.path().join("layers"))
    ));

    // Create a test user
    let hash = auth::hash_password("testpass").unwrap();
    db.create_user("testuser", &hash).unwrap();

    let state = AppState { config, db, storage };

    let app = Router::new()
        .route("/v1/auth/login", post(ph_registry::routes::auth_routes::login))
        .route("/v1/auth/token", post(ph_registry::routes::auth_routes::issue_token))
        .route("/layers/:namespace/:name/versions",
               get(ph_registry::routes::layer_routes::get_versions))
        .route("/layers", get(ph_registry::routes::layer_routes::list_layers))
        .route("/layers/:namespace/:name/:version/:filename",
               get(ph_registry::routes::layer_routes::get_layer_file))
        .route("/layers/:namespace/:name/:version",
               put(ph_registry::routes::layer_routes::put_layer))
        .with_state(state);

    TestServer::new(app).unwrap()
}

/// Minimal valid layer files for test pushes.
fn valid_layer_yaml() -> &'static str {
    "name: expert\nversion: v1.0\nnamespace: base\ndescription: test\ntags: []\nsections: [role]\n"
}

fn valid_prompt_md() -> &'static str {
    "[role]\nYou are an expert.\n"
}

#[tokio::test]
async fn test_list_layers_empty() {
    let tmp = TempDir::new().unwrap();
    let server = build_test_app(&tmp);
    let resp = server.get("/layers").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body, serde_json::json!([]));
}

#[tokio::test]
async fn test_push_and_pull_layer() {
    let tmp = TempDir::new().unwrap();
    let server = build_test_app(&tmp);

    // Push layer.yaml + prompt.md as multipart
    let resp = server
        .put("/layers/base/expert/v1.0")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer phrt_admin"),
        )
        .multipart(
            axum_test::multipart::MultipartForm::new()
                .add_text("layer.yaml", valid_layer_yaml())
                .add_text("prompt.md", valid_prompt_md()),
        )
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);

    // Pull layer.yaml back
    let pull = server.get("/layers/base/expert/v1.0/layer.yaml").await;
    pull.assert_status_ok();
    assert!(pull.text().contains("expert"));
}

#[tokio::test]
async fn test_push_duplicate_returns_409() {
    let tmp = TempDir::new().unwrap();
    let server = build_test_app(&tmp);

    // First push — must succeed with 201
    server
        .put("/layers/base/expert/v1.0")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer phrt_admin"),
        )
        .multipart(
            axum_test::multipart::MultipartForm::new()
                .add_text("layer.yaml", valid_layer_yaml())
                .add_text("prompt.md", valid_prompt_md()),
        )
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Second push — must fail with 409 (versions are immutable)
    server
        .put("/layers/base/expert/v1.0")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer phrt_admin"),
        )
        .multipart(
            axum_test::multipart::MultipartForm::new()
                .add_text("layer.yaml", valid_layer_yaml())
                .add_text("prompt.md", valid_prompt_md()),
        )
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_login_valid_credentials() {
    let tmp = TempDir::new().unwrap();
    let server = build_test_app(&tmp);

    let resp = server
        .post("/v1/auth/login")
        .json(&serde_json::json!({"username": "testuser", "password": "testpass"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(body["token"].as_str().unwrap().starts_with("phrt_"));
}

#[tokio::test]
async fn test_login_wrong_password_returns_401() {
    let tmp = TempDir::new().unwrap();
    let server = build_test_app(&tmp);

    let resp = server
        .post("/v1/auth/login")
        .json(&serde_json::json!({"username": "testuser", "password": "wrongpass"}))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}
