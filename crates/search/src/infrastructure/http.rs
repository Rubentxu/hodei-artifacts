use crate::application::ports::SearchIndex;
use crate::features::basic_search::{handle_basic_search, BasicSearchQuery};
use axum::{    extract::{Extension, Query},    response::Json, routing::get, Router,};
use std::sync::Arc;

#[axum::debug_handler]
pub async fn search_handler(
    Extension(search_index): Extension<Arc<dyn SearchIndex>>,
    Query(query): Query<BasicSearchQuery>,
) -> Result<Json<crate::features::basic_search::BasicSearchResult>, crate::error::SearchError> {
    let result = handle_basic_search(search_index, query).await?;
    Ok(Json(result))
}

pub fn api_router() -> Router {
    Router::new().route("/search", get(search_handler))
}
