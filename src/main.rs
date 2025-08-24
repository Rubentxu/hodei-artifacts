use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    init_tracing();

    let app = Router::new().route("/health", get(health));

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    tracing::info!(%addr, "Servidor iniciado");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .expect("Fallo servidor");
}

async fn health() -> &'static str { "OK" }

fn init_tracing() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .json()
        .try_init();
}

