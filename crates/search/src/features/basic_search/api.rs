use axum::{
    extract::{Query, Extension},
    response::{IntoResponse, Json},
    http::StatusCode,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, error, debug};

use crate::features::basic_search::{
    dto::SearchQuery,
    error::BasicSearchError,
    di::BasicSearchDIContainer,
};

pub struct BasicSearchEndpoint {
    di_container: Arc<BasicSearchDIContainer>,
}

impl BasicSearchEndpoint {
    pub fn new(di_container: Arc<BasicSearchDIContainer>) -> Self {
        Self { di_container }
    }

    pub async fn search(
        Extension(endpoint): Extension<Arc<BasicSearchEndpoint>>,
        query: Query<SearchQuery>,
    ) -> impl IntoResponse {
        debug!("Received search request: {:?}", query);
        
        // Execute the search use case
        match endpoint.di_container.use_case.execute(query.0).await {
            Ok(results) => {
                info!(result_count = results.total_count, "Search completed successfully");
                (StatusCode::OK, Json(results)).into_response()
            }
            Err(e) => {
                error!(error = %e, "Search failed");
                let status_code = match e {
                    BasicSearchError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    BasicSearchError::AuthenticationError(_) => StatusCode::UNAUTHORIZED,
                    BasicSearchError::TimeoutError => StatusCode::REQUEST_TIMEOUT,
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