//! Hodei Artifacts API - Main Entry Point
//!
//! This is the main entry point for the Hodei Artifacts API server.
//! It handles:
//! - Application configuration loading with config-rs
//! - Logging initialization
//! - Bootstrap and dependency injection (composition root)
//! - Axum server setup and routing
//! - Graceful shutdown handling

use hodei_artifacts_api::{
    bootstrap::BootstrapConfig, bootstrap::bootstrap, config::AppConfig, router::build_router,
};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load configuration with config-rs
    let config = match AppConfig::new() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("❌ Configuration error: {}", e);
            eprintln!();
            eprintln!("💡 Configuration tips:");
            eprintln!("   - Set environment variables with HODEI_ prefix");
            eprintln!("   - Create config/default.toml for default values");
            eprintln!("   - Use RUN_MODE=development for development settings");
            eprintln!();
            eprintln!("📖 Example environment variables:");
            eprintln!("   HODEI_SERVER__PORT=3000");
            eprintln!("   HODEI_ROCKSDB__PATH=./data/hodei.rocksdb");
            eprintln!("   HODEI_DATABASE__NAMESPACE=hodei");
            eprintln!("   HODEI_DATABASE__DATABASE=artifacts");
            std::process::exit(1);
        }
    };

    // 2. Initialize logging
    initialize_logging(&config)?;

    info!("🚀 Starting Hodei Artifacts API");
    info!("📋 Configuration loaded");
    info!("   Server: {}", config.server_address());
    info!("   Database: {}", config.database.db_type);
    info!("   RocksDB path: {}", config.rocksdb.path);
    info!("   Schema storage: {}", config.schema.storage_type);
    info!(
        "   IAM schema registration: {}",
        config.schema.register_iam_on_startup
    );

    // 3. Bootstrap application (composition root)
    let bootstrap_config = BootstrapConfig {
        register_iam_schema: config.schema.register_iam_on_startup,
        schema_version: config.schema.version.clone(),
        validate_schemas: config.schema.validate,
    };

    let app_state = bootstrap(&config, bootstrap_config).await.map_err(|e| {
        eprintln!("Bootstrap failed: {}", e);
        std::process::exit(1);
    })?;

    // 4. Build Axum router
    let app = build_router(app_state, &config);

    // 5. Start server
    let listener = tokio::net::TcpListener::bind(config.server_address()).await?;
    let addr = listener.local_addr()?;

    info!("✅ Hodei Artifacts API is ready");
    info!("🌐 Listening on http://{}", addr);
    info!("📊 Health check: http://{}/health", addr);
    info!("📖 API documentation: http://{}/docs", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("👋 Hodei Artifacts API shut down gracefully");
    Ok(())
}

/// Initialize logging based on configuration
fn initialize_logging(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "hodei_artifacts_api={},hodei_iam={},hodei_policies={},kernel={}",
            config.logging.level, config.logging.level, config.logging.level, config.logging.level
        ))
    });

    match config.logging.format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json())
                .init();
        }
        "compact" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().compact())
                .init();
        }
        _ => {
            // Default to "pretty"
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().pretty())
                .init();
        }
    }

    Ok(())
}

/// Graceful shutdown signal handler
///
/// This function listens for shutdown signals (SIGTERM, SIGINT/Ctrl+C)
/// and returns when one is received, triggering graceful shutdown.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            warn!("Received Ctrl+C signal");
        }
        _ = terminate => {
            warn!("Received SIGTERM signal");
        }
    }

    info!("Starting graceful shutdown...");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_validation() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_bootstrap() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.rocksdb");

        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false, // Skip IAM registration for faster tests
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(&config, bootstrap_config).await;
        assert!(result.is_ok(), "Bootstrap should succeed with RocksDB");

        // Clean up
        drop(temp_dir);
    }
}
