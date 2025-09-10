use axum::{
    routing::{post, get, delete, put, IntoMakeService},
    Router,
    Extension,
    handler::Handler,
};
use std::path::PathBuf;

// Import DI containers from the feature crates
use artifact::features::upload_artifact::{UploadArtifactDIContainer, api::UploadArtifactEndpoint};
use artifact::features::upload_artifact::upload_progress::{UploadProgressDIContainer, api::UploadProgressApi};
use iam::features::validate_policy::{ValidatePolicyDIContainer, api::ValidatePolicyApi};
use repository::create_repository_api_module;

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
    let upload_artifact_api = upload_artifact_container.endpoint;

    // --- Feature: Upload Progress Tracking ---
    let upload_progress_container = UploadProgressDIContainer::for_development();
    let upload_progress_api = upload_progress_container.api;

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

    // --- Create and combine Axum routers ---
    let app = Router::new()
        // Artifact endpoints
        .route(
            "/artifacts",
            post(UploadArtifactEndpoint::upload_artifact),
        )
        .route(
            "/uploads/:upload_id/progress",
            get(UploadProgressApi::get_progress),
        )
        .route(
            "/uploads/progress",
            get(UploadProgressApi::list_sessions),
        )
        .route(
            "/uploads/:upload_id/subscribe",
            post(UploadProgressApi::subscribe_client),
        )
        .route(
            "/uploads/:client_id/unsubscribe",
            delete(UploadProgressApi::unsubscribe_client),
        )
        .route(
            "/policies/validate",
            post(ValidatePolicyApi::handle),
        )
        // Repository CRUD endpoints
        .nest("/api/v1", repository_router)
        .layer(Extension(upload_artifact_api))
        .layer(Extension(upload_progress_api))
        .layer(Extension(validate_policy_api));

    // --- Start the server ---
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app.into_make_service()).await.unwrap();
}