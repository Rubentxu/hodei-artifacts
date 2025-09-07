use axum::{
    routing::post,
    Router,
    Extension,
};
// Import the DI container from our new artifact crate
use artifact::features::upload_artifact::{UploadArtifactDIContainer, api::UploadArtifactEndpoint};

#[tokio::main]
async fn main() {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init();

    // 1. Set up DI container for the feature
    // Use production constructor with default configuration
    let upload_artifact_container = UploadArtifactDIContainer::for_production(
        &aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await,
        "mongodb://localhost:27017",
        "hodei",
        "amqp://localhost:5672",
        "events",
        "artifacts"
    ).await;
    let upload_artifact_endpoint = upload_artifact_container.endpoint;

    // 2. Create Axum router and add the feature's route
    let app = Router::new()
        .route(
            "/artifacts",
            post(UploadArtifactEndpoint::upload_artifact),
        )
        .layer(Extension(upload_artifact_endpoint));

    // 3. Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}