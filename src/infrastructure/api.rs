use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use repository::infrastructure::http::create_repository_handler;
use search::infrastructure::http::search_handler;
use axum::Extension;

pub async fn create_router(app_state: Arc<Mutex<AppState>>) -> Router {
    let state = app_state.lock().await;
    let repo_store = state.repo_store.clone();
    let search_index = state.search_index.clone();

    Router::new()
        .route("/health", get(health))
        .route("/v1/repositories", post(create_repository_handler))
        .layer(Extension(repo_store))
        .route("/v1/search", get(search_handler))
        .layer(Extension(search_index))
}

/// Healthcheck simple.
async fn health() -> &'static str {
    "OK"
}
