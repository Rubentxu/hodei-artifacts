use axum::{routing::get, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    init_tracing();

    // Define the application routes
    let app = Router::new().route("/health", get(health));

    // Define the address and port to listen on
    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();

    // Create a TCP listener bound to the address
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind to address");

    tracing::info!(addr = %listener.local_addr().unwrap(), "Server started");

    // Run the server using axum::serve
    axum::serve(listener, app)
        .await
        .expect("Server failed");
}

/// A simple health check handler that returns "OK"
async fn health() -> &'static str {
    "OK"
}

/// Initializes the tracing subscriber for logging.
/// Logs are formatted as JSON.
fn init_tracing() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .json()
        .try_init();
}
