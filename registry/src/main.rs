mod config;
mod db;
mod error;
mod storage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = std::env::args().nth(1)
        .unwrap_or_else(|| "registry.yaml".to_string());
    let _config = config::RegistryConfig::load(&config_path)?;
    println!("ph-registry starting (stub)");
    Ok(())
}
