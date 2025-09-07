use std::sync::Arc;
use axum::{
    routing::post,
    Router,
};
// Import the DI container from our new artifact crate
use artifact::features::upload_artifact::UploadArtifactDIContainer;

#[tokio::main]
async fn main() {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init();

    // 1. Set up DI container for the feature
    // We use the testing constructor for now, which wires up the mock adapters.
    let upload_artifact_container = UploadArtifactDIContainer::for_testing();
    let upload_artifact_endpoint = upload_artifact_container.endpoint;

    // 2. Create Axum router and add the feature's route
    let app = Router::new()
        .route(
            "/artifacts",
            // The handler needs to be a function or a closure that captures the state.
            // We clone the Arc<Endpoint> so each request has its own reference to it.
            post(move |multipart| {
                let endpoint = Arc::clone(&upload_artifact_endpoint);
                async move {
                    // The handle_request method is defined on the endpoint struct
                    endpoint.handle_request(multipart).await
                }
            }),
        );

    // 3. Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}