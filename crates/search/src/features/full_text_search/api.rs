use axum::{
    extract::{Query, Extension},
    response::{IntoResponse, Json},
    http::StatusCode,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, debug, error};

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    use_case::FullTextSearchUseCase,
};

pub struct FullTextSearchEndpoint {
    use_case: Arc<FullTextSearchUseCase>,
}

impl FullTextSearchEndpoint {
    pub fn new(use_case: Arc<FullTextSearchUseCase>) -> Self {
        Self { use_case }
    }

    pub async fn search(
        Extension(endpoint): Extension<Arc<FullTextSearchEndpoint>>,
        query: Query<SearchQuery>,
    ) -> impl IntoResponse {
        debug!(query = %query.q, "Received search request");
        
        // Execute the search use case
        match endpoint.use_case.execute(query.0).await {
            Ok(results) => {
                info!(result_count = results.total_count, "Search completed successfully");
                (StatusCode::OK, Json(results)).into_response()
            }
            Err(e) => {
                error!(error = %e, "Search failed");
                let status_code = match e {
                    FullTextSearchError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    FullTextSearchError::AuthenticationError(_) => StatusCode::UNAUTHORIZED,
                    FullTextSearchError::TimeoutError => StatusCode::REQUEST_TIMEOUT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                
                let error_response = json!({
                    "error": e.to_string(),
                });
                
                (status_code, Json(error_response)).into_response()
            }
        }
    }
    
    // We could also implement a POST endpoint for more complex search queries
    // but for the basic search engine, the GET endpoint should be sufficient
}