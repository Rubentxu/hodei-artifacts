use hodei_artifacts_api::config::Config;
use hodei_artifacts_api::{build_app_state, build_router};
use hodei_artifacts_api::error::{AppError, Result};
use hodei_artifacts_api::services::shutdown;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::from_env()?;

    // Setup logging
    setup_logging(&config)?;

    tracing::info!("Starting Policy BaaS MVP");
    tracing::debug!("Configuration: {:#?}", config);

    // Initialize metrics if enabled
    if config.metrics.enabled && config.metrics.prometheus_registry {
        initialize_metrics();
    }

    // Build shared application state via lib
    let state = build_app_state(&config).await?;

    // Build application router
    let app = build_router(state.clone()).await?;

    // Start server
    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(AppError::ServerBind)?;

    tracing::info!("Server listening on {}", bind_addr);
    tracing::info!("Health check available at: http://{}/health", bind_addr);
    tracing::info!(
        "OpenAPI spec available at: http://{}/api-docs/openapi.json",
        bind_addr
    );
    if config.metrics.enabled {
        tracing::info!(
            "Metrics available at: http://{}{}",
            bind_addr,
            config.metrics.endpoint
        );
    }

    // Start server with graceful shutdown
    let shutdown_timeout = Duration::from_secs(config.server.shutdown_timeout_seconds);

    tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                tracing::error!("Server error: {}", e);
                return Err(AppError::Internal(e.to_string()));
            }
        }
        _ = shutdown::graceful_shutdown(shutdown_timeout) => {
            tracing::info!("Received shutdown signal, stopping server");
        }
    }

    tracing::info!("Application shutdown completed");
    Ok(())
}

fn setup_logging(config: &Config) -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_new(&config.logging.level)
        .map_err(|e| AppError::LoggingSetup(e.to_string()))?;

    let subscriber = tracing_subscriber::registry().with(filter);

    match config.logging.format {
        hodei_artifacts_api::config::LogFormat::Json => {
            subscriber
                .with(tracing_subscriber::fmt::layer().json())
                .try_init()
                .map_err(|e| AppError::LoggingSetup(e.to_string()))?;
        }
        hodei_artifacts_api::config::LogFormat::Pretty => {
            subscriber
                .with(tracing_subscriber::fmt::layer().pretty())
                .try_init()
                .map_err(|e| AppError::LoggingSetup(e.to_string()))?;
        }
        hodei_artifacts_api::config::LogFormat::Compact => {
            subscriber
                .with(tracing_subscriber::fmt::layer().compact())
                .try_init()
                .map_err(|e| AppError::LoggingSetup(e.to_string()))?;
        }
    }

    Ok(())
}

fn initialize_metrics() {
    // Initialize Prometheus metrics registry
    tracing::info!("Metrics registry initialized");
}
