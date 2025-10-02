use crate::features::validate_policy::dto::{ValidatePolicyRequest, ValidatePolicyResponse};
use crate::features::validate_policy::use_case::ValidatePolicyUseCase;
use axum::{
    Json,
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

// The API endpoint for policy validation.
pub struct ValidatePolicyApi {
    pub use_case: Arc<ValidatePolicyUseCase>,
}

impl ValidatePolicyApi {
    pub fn new(use_case: Arc<ValidatePolicyUseCase>) -> Self {
        Self { use_case }
    }

    // The Axum handler function.
    pub async fn handle(
        Extension(state): Extension<Arc<Self>>,
        Json(request): Json<ValidatePolicyRequest>,
    ) -> Response {
        match state.use_case.execute(request).await {
            Ok(response) => {
                if response.is_valid {
                    (StatusCode::OK, Json(response)).into_response()
                } else {
                    (StatusCode::BAD_REQUEST, Json(response)).into_response()
                }
            }
            Err(e) => {
                // Internal errors should be a 500
                let error_response = ValidatePolicyResponse {
                    is_valid: false,
                    errors: vec![format!("Internal server error: {}", e)],
                };
                (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
            }
        }
    }
}
