mod api;
mod app_state;
mod config;
mod error;
mod middleware;
mod services;

use crate::{
    app_state::{AppMetrics, AppState, HealthStatus},
    config::Config,
    error::{AppError, Result},
    services::shutdown,
};
use axum::{
    Router,
    routing::{delete, get, post, put},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

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

    // Build policies use cases via DI from policies crate
    // Using mem storage for development (change to embedded for production)
    tracing::info!("Initializing policies use cases...");

    #[cfg(feature = "embedded")]
    let (create_policy_uc, authorization_engine) =
        policies::features::create_policy::di::embedded::make_use_case_embedded(
            &config.database.url,
        )
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build create_policy use case (embedded): {}",
                e
            ))
        })?;
    #[cfg(not(feature = "embedded"))]
    let (create_policy_uc, authorization_engine) =
        policies::features::create_policy::di::make_use_case_mem()
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to build create_policy use case (mem): {}",
                    e
                ))
            })?;

    #[cfg(feature = "embedded")]
    let (get_policy_uc, _) =
        policies::features::get_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to build get_policy use case (embedded): {}",
                    e
                ))
            })?;
    #[cfg(not(feature = "embedded"))]
    let (get_policy_uc, _) = policies::features::get_policy::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!("failed to build get_policy use case (mem): {}", e))
        })?;

    #[cfg(feature = "embedded")]
    let (list_policies_uc, _) =
        policies::features::list_policies::di::embedded::make_use_case_embedded(
            &config.database.url,
        )
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build list_policies use case (embedded): {}",
                e
            ))
        })?;
    #[cfg(not(feature = "embedded"))]
    let (list_policies_uc, _) = policies::features::list_policies::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build list_policies use case (mem): {}",
                e
            ))
        })?;

    // Build update_policy use case
    #[cfg(feature = "embedded")]
    let (update_policy_uc, _) =
        policies::features::update_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to build update_policy use case (embedded): {}",
                    e
                ))
            })?;
    #[cfg(not(feature = "embedded"))]
    let (update_policy_uc, _) = policies::features::update_policy::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build update_policy use case (mem): {}",
                e
            ))
        })?;

    // Build validate_policy use case
    #[cfg(feature = "embedded")]
    let (validate_policy_uc, _) =
        policies::features::validate_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to build validate_policy use case (embedded): {}",
                    e
                ))
            })?;
    #[cfg(not(feature = "embedded"))]
    let (validate_policy_uc, _) = policies::features::validate_policy::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build validate_policy use case (mem): {}",
                e
            ))
        })?;

    // Build policy_playground use case
    #[cfg(feature = "embedded")]
    let (policy_playground_uc, _) =
        policies::features::policy_playground::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to build policy_playground use case (embedded): {}",
                    e
                ))
            })?;
    #[cfg(not(feature = "embedded"))]
    let (policy_playground_uc, _) = policies::features::policy_playground::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build policy_playground use case (mem): {}",
                e
            ))
        })?;

    #[cfg(feature = "embedded")]
    let (delete_policy_uc, _) =
        policies::features::delete_policy::di::embedded::make_use_case_embedded(
            &config.database.url,
        )
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build delete_policy use case (embedded): {}",
                e
            ))
        })?;
    #[cfg(not(feature = "embedded"))]
    let (delete_policy_uc, _) = policies::features::delete_policy::di::make_use_case_mem()
        .await
        .map_err(|e| {
            AppError::Internal(format!(
                "failed to build delete_policy use case (mem): {}",
                e
            ))
        })?;

    tracing::info!("Policy engine and use cases initialized successfully");

    // Build analyze policies and batch eval use cases (mem)
    let analyze_policies_uc = policies::features::policy_analysis::di::make_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build analyze_policies use case (mem): {}", e)))?;

    let batch_eval_uc = policies::features::batch_eval::di::make_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build batch_eval use case (mem): {}", e)))?;

    // Create shared application state
    let shared_state = Arc::new(AppState {
        config: config.clone(),
        metrics,
        health: Arc::new(RwLock::new(HealthStatus::new())),
        create_policy_uc: Arc::new(create_policy_uc),
        get_policy_uc: Arc::new(get_policy_uc),
        list_policies_uc: Arc::new(list_policies_uc),
        delete_policy_uc: Arc::new(delete_policy_uc),
        update_policy_uc: Arc::new(update_policy_uc),
        validate_policy_uc: Arc::new(validate_policy_uc),
        policy_playground_uc: Arc::new(policy_playground_uc),
        analyze_policies_uc: Arc::new(analyze_policies_uc),
        batch_eval_uc: Arc::new(batch_eval_uc),
        authorization_engine,
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
    tracing::info!("Metrics registry initialized");
}

async fn build_router(state: Arc<AppState>) -> Result<Router> {
    let cors = build_cors_layer(&state.config)?;

    let request_timeout = Duration::from_secs(state.config.server.request_timeout_seconds);

    // Define OpenAPI documentation
    #[derive(OpenApi)]
    #[openapi(
        paths(
            // Policy management endpoints
            api::create_policy,
            api::list_policies,
            api::get_policy,
            api::delete_policy,
            api::update_policy,
            api::validate_policy,
            api::policy_playground,
        ),
        components(
            schemas(
                api::policy_handlers::PolicyResponse,
                api::policy_handlers::CreatePolicyRequest,
                api::policy_handlers::UpdatePolicyRequest,
                api::policy_handlers::ValidatePolicyRequest,
                api::policy_handlers::PlaygroundRequestApi,
                api::policy_handlers::PlaygroundScenarioApi,
                api::policy_handlers::PlaygroundOptionsApi,
                api::policy_handlers::PlaygroundResponseApi,
                api::policy_handlers::PlaygroundAuthResultApi,
                api::policy_handlers::PolicyListResponse,
                api::policy_handlers::ListPoliciesParams,
                api::policy_handlers::ErrorResponse
            )
        ),
        tags(
            (name = "policies", description = "Policy management endpoints - Create, read, update, and delete Cedar policies"),
            (name = "health", description = "Health check endpoints"),
            (name = "authorization", description = "Authorization endpoints")
        ),
        info(
            title = "Hodei Artifacts Policy API",
            version = "1.0.0",
            description = "REST API for managing Cedar policies and authorization. This API provides endpoints for creating and retrieving policies using the Cedar policy language.",
            contact(
                name = "API Support",
                email = "support@hodei.io"
            ),
            license(
                name = "MIT",
            )
        )
    )]
    struct ApiDoc;

    let api_routes = Router::new()
        // Policy management routes
        .route("/policies", post(api::create_policy))
        .route("/policies", get(api::list_policies))
        .route("/policies/{id}", get(api::get_policy))
        .route("/policies/{id}", delete(api::delete_policy))
        .route("/policies/{id}", put(api::update_policy))
        .route("/policies/validate", post(api::validate_policy))
        .route("/policies/playground", post(api::policy_playground))
        .route("/policies/analysis", post(api::analyze_policies))
        .route("/policies/playground/batch", post(api::batch_playground))
        // Authorization route
        .route("/authorize", post(api::authorize));

    let health_routes = Router::new()
        .route("/health", get(api::health))
        .route("/ready", get(api::readiness));

    // Build main app router with state
    let mut app_router = Router::new()
        .nest("/api/v1", api_routes)
        .merge(health_routes);

    // Add metrics endpoint if enabled
    if state.config.metrics.enabled {
        app_router = app_router.route(&state.config.metrics.endpoint, get(api::metrics));
    }

    // Apply state and middleware
    let app = app_router
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::metrics_middleware,
        ))
        .layer(axum::middleware::from_fn(middleware::logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(request_timeout))
        .layer(CompressionLayer::new())
        .layer(cors);

    // Merge Swagger UI router using the recommended approach for Axum 0.8
    let final_app =
        app.merge(SwaggerUi::new("/api-docs").url("/api-docs/openapi.json", ApiDoc::openapi()));

    Ok(final_app)
}

fn build_cors_layer(config: &Config) -> Result<CorsLayer> {
    let mut cors = CorsLayer::new();

    // Configure allowed origins
    if config.cors.allow_origins.contains(&"*".to_string()) {
        cors = cors.allow_origin(tower_http::cors::Any);
    } else {
        for origin in &config.cors.allow_origins {
            cors = cors.allow_origin(origin.parse::<http::HeaderValue>().map_err(|_| {
                AppError::Configuration(format!("Invalid CORS origin: {}", origin))
            })?);
        }
    }

    // Configure allowed headers
    let headers: std::result::Result<Vec<_>, AppError> = config
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
    let methods: std::result::Result<Vec<_>, AppError> = config
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
