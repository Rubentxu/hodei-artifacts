use axum::{routing::get, Router};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use repository::infrastructure::http::RepositoryApi;
use search::infrastructure::http::SearchApi;
use iam::infrastructure::http::Api as IamApi;

pub async fn create_router(app_state: Arc<Mutex<AppState>>) -> Router {
    let state = app_state.lock().await;
    let repo_store = state.repo_store.clone();
    let search_index = state.search_index.clone();
    let iam_api = state.iam_api.clone();

    let repo_api = RepositoryApi::new(repo_store);
    let search_api = SearchApi::new(search_index);
    let iam_api = IamApi::new(iam_api);

    Router::new()
        .route("/health", get(health))
        .merge(repo_api.routes())
        .merge(search_api.routes())
        .merge(iam_api.routes())
}

/// Healthcheck simple.
async fn health() -> &'static str {
    "OK"
}
