pub mod api;
pub mod app_state;
pub mod config;
pub mod error;
pub mod middleware;
pub mod services;

use crate::app_state::{AppMetrics, AppState, HealthStatus};
use crate::config::Config;
use crate::error::{AppError, Result};
use axum::{routing::{delete, get, post, put}, Router};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Build AppState using either embedded or in-memory implementations depending on features
pub async fn build_app_state(config: &Config) -> Result<Arc<AppState>> {
    // Metrics (simple initialization)
    let metrics = AppMetrics::new();

    // Policies DI
    #[cfg(feature = "embedded")]
    let (create_policy_uc, authorization_engine) = policies::features::create_policy::di::embedded::make_create_policy_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build create_policy use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (create_policy_uc, authorization_engine) = policies::features::create_policy::di::make_create_policy_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build create_policy use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (get_policy_uc, _) = policies::features::get_policy::di::embedded::make_get_policy_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build get_policy use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (get_policy_uc, _) = policies::features::get_policy::di::make_get_policy_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build get_policy use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (list_policies_uc, _) = policies::features::list_policies::di::embedded::make_list_policies_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build list_policies use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (list_policies_uc, _) = policies::features::list_policies::di::make_list_policies_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build list_policies use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (update_policy_uc, _) = policies::features::update_policy::di::embedded::make_update_policy_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build update_policy use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (update_policy_uc, _) = policies::features::update_policy::di::make_update_policy_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build update_policy use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (validate_policy_uc, _) = policies::features::validate_policy::di::embedded::make_validate_policy_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build validate_policy use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (validate_policy_uc, _) = policies::features::validate_policy::di::make_validate_policy_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build validate_policy use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (policy_playground_uc, _) = policies::features::policy_playground::di::embedded::make_policy_playground_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build policy_playground use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (policy_playground_uc, _) = policies::features::policy_playground::di::make_policy_playground_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build policy_playground use case (mem): {}", e)))?;

    let analyze_policies_uc = policies::features::policy_analysis::di::make_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build analyze_policies use case (mem): {}", e)))?;

    let batch_eval_uc = policies::features::batch_eval::di::make_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build batch_eval use case (mem): {}", e)))?;

    #[cfg(feature = "embedded")]
    let (delete_policy_uc, _) = policies::features::delete_policy::di::embedded::make_delete_policy_use_case_embedded(&config.database.url)
        .await
        .map_err(|e| AppError::Internal(format!("failed to build delete_policy use case (embedded): {}", e)))?;
    #[cfg(not(feature = "embedded"))]
    let (delete_policy_uc, _) = policies::features::delete_policy::di::make_delete_policy_use_case_mem()
        .await
        .map_err(|e| AppError::Internal(format!("failed to build delete_policy use case (mem): {}", e)))?;

    // IAM in-memory repos and use cases (for now always in-memory)
    let user_repo = Arc::new(hodei_iam::shared::infrastructure::persistence::InMemoryUserRepository::new());
    let group_repo = Arc::new(hodei_iam::shared::infrastructure::persistence::InMemoryGroupRepository::new());
    let create_user_uc = hodei_iam::features::create_user::di::make_use_case(user_repo.clone());
    let create_group_uc = hodei_iam::features::create_group::di::make_use_case(group_repo.clone());
    let add_user_to_group_uc = hodei_iam::features::add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let state = Arc::new(AppState {
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
        // IAM
        create_user_uc: Arc::new(create_user_uc),
        create_group_uc: Arc::new(create_group_uc),
        add_user_to_group_uc: Arc::new(add_user_to_group_uc),
        user_repo,
        group_repo,
    });

    Ok(state)
}

/// Build a Router suitable for tests using in-memory DI
pub async fn build_app_for_tests() -> Result<Router> {
    let config = config::Config::from_env()?;
    let state = build_app_state(&config).await?;
    build_router(state).await
}

/// Public router builder for reuse in tests
pub async fn build_router(state: Arc<AppState>) -> Result<Router> {
    let cors = build_cors_layer(&state.config)?;
    let request_timeout = std::time::Duration::from_secs(state.config.server.request_timeout_seconds);

    #[derive(OpenApi)]
    #[openapi(
        paths(
            api::create_policy,
            api::list_policies,
            api::get_policy,
            api::delete_policy,
            api::update_policy,
            api::validate_policy,
            api::policy_playground,
            api::analyze_policies,
            api::batch_playground,
            // IAM endpoints
            api::create_user,
            api::list_users,
            api::create_group,
            api::list_groups,
            api::add_user_to_group,
        ),
        components(
            schemas(
                api::policy_handlers::PolicyResponse,
                api::policy_handlers::CreatePolicyRequest,
                api::policy_handlers::UpdatePolicyRequest,
                api::policy_handlers::ValidatePolicyRequest,
                api::policy_handlers::PlaygroundRequestApi,
                api::policy_handlers::PlaygroundScenarioApi,
                api::policy_handlers::EntityDefinitionApi,
                api::policy_handlers::PlaygroundOptionsApi,
                api::policy_handlers::PlaygroundResponseApi,
                api::policy_handlers::ValidationErrorApi,
                api::policy_handlers::ValidationWarningApi,
                api::policy_handlers::PolicyValidationApi,
                api::policy_handlers::SchemaValidationApi,
                api::policy_handlers::EvaluationStatisticsApi,
                api::policy_handlers::AnalyzePoliciesRequestApi,
                api::policy_handlers::AnalyzePoliciesResponseApi,
                api::policy_handlers::BatchPlaygroundRequestApi,
                api::policy_handlers::BatchPlaygroundResponseApi,
                api::policy_handlers::PolicyListResponse,
                api::policy_handlers::ListPoliciesParams,
                api::policy_handlers::ErrorResponse,
                // IAM schemas
                api::CreateUserRequest,
                api::UserResponse,
                api::CreateGroupRequest,
                api::GroupResponse,
                api::AddUserToGroupRequest,
            )
        ),
        tags(
            (name = "policies", description = "Policy management endpoints - Create, read, update, and delete Cedar policies"),
            (name = "health", description = "Health check endpoints"),
            (name = "authorization", description = "Authorization endpoints"),
            (name = "IAM", description = "Identity and Access Management endpoints")
        )
    )]
    struct ApiDoc;

    let api_routes = Router::new()
        .route("/policies", post(api::create_policy))
        .route("/policies", get(api::list_policies))
        .route("/policies/{id}", get(api::get_policy))
        .route("/policies/{id}", delete(api::delete_policy))
        .route("/policies/{id}", put(api::update_policy))
        .route("/policies/validate", post(api::validate_policy))
        .route("/policies/playground", post(api::policy_playground))
        .route("/policies/analysis", post(api::analyze_policies))
        .route("/policies/playground/batch", post(api::batch_playground))
        .route("/authorize", post(api::authorize))
        // IAM routes nested under /api/v1/iam
        .nest("/iam", api::iam_routes());

    let health_routes = Router::new()
        .route("/health", get(api::health))
        .route("/ready", get(api::readiness));

    let mut app_router = Router::new()
        .nest("/api/v1", api_routes)
        .merge(health_routes);

    if state.config.metrics.enabled {
        app_router = app_router.route(&state.config.metrics.endpoint, get(api::metrics));
    }

    let app = app_router
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::metrics_middleware))
        .layer(axum::middleware::from_fn(middleware::logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(request_timeout))
        .layer(CompressionLayer::new())
        .layer(cors);

    let final_app = app.merge(SwaggerUi::new("/api-docs").url("/api-docs/openapi.json", ApiDoc::openapi()));
    Ok(final_app)
}

fn build_cors_layer(config: &config::Config) -> Result<CorsLayer> {
    let mut cors = CorsLayer::new();
    if config.cors.allow_origins.contains(&"*".to_string()) {
        cors = cors.allow_origin(tower_http::cors::Any);
    } else {
        for origin in &config.cors.allow_origins {
            cors = cors.allow_origin(origin.parse::<http::HeaderValue>().map_err(|_| AppError::Configuration(format!("Invalid CORS origin: {}", origin)))?);
        }
    }
    Ok(cors)
}
