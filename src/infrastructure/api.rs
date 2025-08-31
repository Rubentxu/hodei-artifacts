use axum::{routing::get, Extension, Router};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use iam::infrastructure::http::Api as IamApi;
use repository::infrastructure::http::api_router as repository_router;
use search::infrastructure::http::api_router as search_router;
use distribution::infrastructure::http::create_distribution_router;

pub async fn create_router(app_state: Arc<Mutex<AppState>>) -> Router {
    let state = app_state.lock().await;
    let repo_store = state.repo_store.clone();
    let search_index = state.search_index.clone();
    let iam_api = state.iam_api.clone();
    let artifact_repository = state.artifact_repository.clone();
    let artifact_storage = state.artifact_storage.clone();
    let artifact_event_publisher = state.artifact_event_publisher.clone();

    let iam_api_router = IamApi::new(iam_api.clone()).routes(); // Clone iam_api for distribution router

    let authorization = state.authorization.clone();
    let distribution_router = create_distribution_router(
        artifact_storage,
        artifact_repository,
        artifact_event_publisher,
        authorization,
    );

    Router::new()
        .route("/health", get(health))
        .merge(repository_router())
        .merge(search_router())
        .merge(iam_api_router)
        .merge(distribution_router) // Merge the new distribution router
        .layer(Extension(repo_store))
        .layer(Extension(search_index))
}

/// Healthcheck simple.
async fn health() -> &'static str {
    "OK"
}
