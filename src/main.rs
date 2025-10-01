mod adapters;
mod api;
mod app_state;
mod config;
mod error;
mod middleware;
mod models;
mod ports;
mod services;

use crate::{
    adapters::SurrealDbAdapter,
    app_state::{AppMetrics, AppState, HealthStatus},
    config::Config,
    error::{AppError, Result},
    models::{BlogPost, Team, User},
    ports::{AuthorizationEnginePort, PolicyStorePort, StorageAdapterPort},
    services::shutdown,
};
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use http::{HeaderValue, Method};
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
    
    // Initialize metrics collector
    let metrics = AppMetrics::new();
    
    // Connect to database with retry logic
    let storage = connect_database(&config).await?;
    tracing::info!("Database connection established");
    
    // Build policy engine
    let (engine, policy_store) = build_policy_engine(storage).await?;
    tracing::info!("Policy engine initialized successfully");
    
    // Create shared application state
    let shared_state = Arc::new(AppState {
        engine,
        policy_store,
        storage,
        config: config.clone(),
        metrics,
        health: Arc::new(RwLock::new(HealthStatus::new())),
    });
    
    // Build application router
    let app = build_router(shared_state.clone()).await?;
    
    // Start server
    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(AppError::ServerBind)?;
    
    tracing::info!("Server listening on {}", bind_addr);
    tracing::info!("Health check available at: http://{}/health", bind_addr);
    if config.metrics.enabled {
        tracing::info!("Metrics available at: http://{}{}", bind_addr, config.metrics.endpoint);
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
        config::LogFormat::Json => {
            subscriber
                .with(tracing_subscriber::fmt::layer().json())
                .try_init()
                .map_err(|e| AppError::LoggingSetup(e.to_string()))?;
        }
        config::LogFormat::Pretty => {
            subscriber
                .with(tracing_subscriber::fmt::layer().pretty())
                .try_init()
                .map_err(|e| AppError::LoggingSetup(e.to_string()))?;
        }
        config::LogFormat::Compact => {
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
    // This would depend on your specific metrics implementation
    tracing::info!("Metrics registry initialized");
}

async fn connect_database(config: &Config) -> Result<Arc<dyn StorageAdapterPort>> {
    let mut attempts = 0;
    let max_attempts = config.database.retry_attempts;
    let base_delay = Duration::from_secs(1);
    
    loop {
        match SurrealDbAdapter::connect(&config.database.url).await {
            Ok(adapter) => {
                tracing::info!("Successfully connected to database");
                return Ok(Arc::new(adapter));
            }
            Err(e) => {
                attempts += 1;
                if attempts >= max_attempts {
                    return Err(AppError::DatabaseConnection(format!(
                        "Failed to connect after {} attempts: {}",
                        max_attempts, e
                    )));
                }
                
                let delay = base_delay * 2_u32.pow(attempts - 1);
                tracing::warn!(
                    "Database connection attempt {} failed, retrying in {:?}: {}",
                    attempts,
                    delay,
                    e
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn build_policy_engine(
    storage: Arc<dyn StorageAdapterPort>,
) -> Result<(Arc<dyn AuthorizationEnginePort>, Arc<dyn PolicyStorePort>)> {
    // En una implementación real, aquí construiríamos el motor de autorización y el almacenamiento de políticas
    // basado en el adaptador de almacenamiento proporcionado
    Ok((Arc::new(SurrealDbAdapter::new()), Arc::new(SurrealDbAdapter::new())))
}

async fn build_router(state: Arc<AppState>) -> Result<Router> {
    let cors = build_cors_layer(&state.config)?;
    
    let request_timeout = Duration::from_secs(state.config.server.request_timeout_seconds);
    
    let api_routes = Router::new()
        // Policy management routes
        .route("/policies", post(api::create_policy))
        .route("/policies", get(api::list_policies))
        .route("/policies/:id", delete(api::delete_policy))
        
        // Authorization route
        .route("/authorize", post(api::authorize));
    
    let health_routes = Router::new()
        .route("/health", get(api::health))
        .route("/ready", get(api::readiness));
    
    let mut app = Router::new()
        .nest("/api/v1", api_routes)
        .merge(health_routes)
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::metrics_middleware))
        .layer(axum::middleware::from_fn(middleware::logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(request_timeout))
        .layer(CompressionLayer::new())
        .layer(cors);
    
    // Add metrics endpoint if enabled
    if state.config.metrics.enabled {
        app = app.route(&state.config.metrics.endpoint, get(api::metrics));
    }
    
    Ok(app)
}

fn build_cors_layer(config: &Config) -> Result<CorsLayer> {
    let mut cors = CorsLayer::new();
    
    // Configure allowed origins
    if config.cors.allow_origins.contains(&"*".to_string()) {
        cors = cors.allow_origin(tower_http::cors::Any);
    } else {
        for origin in &config.cors.allow_origins {
            cors = cors.allow_origin(
                origin
                    .parse::<http::HeaderValue>()
                    .map_err(|_| AppError::Configuration(format!("Invalid CORS origin: {}", origin)))?,
            );
        }
    }
    
    // Configure allowed headers
    let headers: Result<Vec<_>, _> = config
        .cors
        .allow_headers
        .iter()
        .map(|h| {
            h.parse::<http::HeaderName>()
                .map_err(|_| AppError::Configuration(format!("Invalid CORS header: {}", h)))
        })
        .collect();
    cors = cors.allow_headers(headers?);
    
    // Configure allowed methods
    let methods: Result<Vec<_>, _> = config
        .cors
        .allow_methods
        .iter()
        .map(|m| {
            m.parse::<http::Method>()
                .map_err(|_| AppError::Configuration(format!("Invalid CORS method: {}", m)))
        })
        .collect();
    cors = cors.allow_methods(methods?);
    
    // Configure max age
    if let Some(max_age) = config.cors.max_age {
        cors = cors.max_age(Duration::from_secs(max_age));
    }
    
    Ok(cors)
}
