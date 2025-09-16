use axum::{
    routing::{delete, get, post, put, IntoMakeService},
    Extension, Router,
};
use std::path::PathBuf;

// Import DI containers from the feature crates
use artifact::features::upload_artifact::UploadArtifactDIContainer;
use artifact::features::upload_progress::UploadProgressDIContainer;
use iam::features::validate_policy::{ValidatePolicyDIContainer, api::ValidatePolicyApi};
use repository::create_repository_api_module;
// Import root API handlers
mod api;
use crate::api::upload_artifact::handlers::upload_artifact_handler;
use crate::api::upload_progress::handlers as progress_handlers;
use crate::api::validation_engine::handlers as validation_handlers;
use crate::api::versioning::handlers as versioning_handlers;
use artifact::features::validation_engine::di::ValidationEngineDIContainer;
use artifact::features::versioning::di::VersioningDIContainer;
use artifact::features::upload_artifact::adapter::LocalFsArtifactStorage;

#[tokio::main]
async fn main() {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init();

    // --- Feature: Upload Artifact ---
    let upload_artifact_container = UploadArtifactDIContainer::for_production(
        &aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await,
        "mongodb://localhost:27017",
        "hodei",
        "amqp://localhost:5672",
        "events",
        "artifacts"
    ).await;
    let upload_artifact_use_case = upload_artifact_container.use_case.clone();

    // --- Feature: Upload Progress Tracking ---
    let upload_progress_container = UploadProgressDIContainer::for_development();
    let upload_progress_use_case = upload_progress_container.use_case.clone();

    // --- Feature: Validate Policy ---
    let schema_path = PathBuf::from("crates/security/schema/policy_schema.cedarschema");
    let validate_policy_container = ValidatePolicyDIContainer::for_production(schema_path)
        .expect("Failed to create ValidatePolicyDIContainer");
    let validate_policy_api = validate_policy_container.api;

    // --- Repository CRUD Endpoints ---
    let repository_api_module = create_repository_api_module(
        mongodb::Client::with_uri_str("mongodb://localhost:27017")
            .await
            .expect("Failed to connect to MongoDB")
            .database("hodei")
    );
    let repository_router = repository_api_module.create_router();

    // --- Feature: Versioning ---
    let versioning_use_case = VersioningDIContainer::new_with_real_implementations().into_use_case();

    // --- Feature: Validation Engine ---
    let validation_storage_dir = std::env::var("HODEI_STORAGE_DIR").unwrap_or_else(|_| "/tmp/hodei-storage".to_string());
    let validation_storage = std::sync::Arc::new(LocalFsArtifactStorage::new(PathBuf::from(validation_storage_dir)));
    let validation_use_case = ValidationEngineDIContainer::new_with_real_implementations(validation_storage).into_use_case();

    // --- Create and combine Axum routers ---
    let app = Router::new()
        // Artifact endpoints
        .route("/artifacts", post(upload_artifact_handler))
        .route("/uploads/{upload_id}/progress", get(progress_handlers::get_progress))
        .route("/uploads/progress", get(progress_handlers::list_sessions))
        .route(
            "/uploads/{upload_id}/subscribe",
            post(progress_handlers::subscribe_client),
        )
        .route(
            "/uploads/{client_id}/unsubscribe",
            delete(progress_handlers::unsubscribe_client),
        )
        .route(
            "/policies/validate",
            post(ValidatePolicyApi::handle),
        )
        // Versioning endpoints
        .route("/versioning/validate", post(versioning_handlers::validate_version))
        .route(
            "/versioning/config/{repository_hrn}",
            get(versioning_handlers::get_versioning_config).put(versioning_handlers::update_versioning_config),
        )
        .route(
            "/versioning/versions/{package_hrn}",
            get(versioning_handlers::get_existing_versions),
        )
        // Validation engine endpoints
        .route(
            "/validation/validate",
            post(validation_handlers::validate_artifact),
        )
        .route(
            "/validation/rules/{artifact_type}",
            get(validation_handlers::get_validation_rules),
        )
        .route(
            "/validation/rules",
            post(validation_handlers::add_validation_rule),
        )
        .route(
            "/validation/rules/{rule_id}",
            delete(validation_handlers::remove_validation_rule),
        )
        // Repository CRUD endpoints
        .nest("/api/v1", repository_router)
        .layer(Extension(upload_artifact_use_case))
        .layer(Extension(upload_progress_use_case))
        .layer(Extension(versioning_use_case))
        .layer(Extension(validation_use_case))
        .layer(Extension(validate_policy_api));

    // --- Start the server ---
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app.into_make_service()).await.unwrap();
}