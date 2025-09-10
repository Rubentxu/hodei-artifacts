use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use crate::infrastructure::errors::IamError;
use super::dto::*;
use super::use_case::AnalyzePolicyCoverageUseCasePort;

#[derive(Debug, Deserialize)]
pub struct AnalyzeCoverageQueryParams {
    pub include_suggestions: Option<bool>,
    pub schema_version: Option<String>,
}

pub async fn analyze_policy_coverage_handler(
    State(use_case): State<Arc<dyn AnalyzePolicyCoverageUseCasePort>>,
    Query(params): Query<AnalyzeCoverageQueryParams>,
    Json(mut request): Json<AnalyzeCoverageRequest>,
) -> Result<Json<AnalyzeCoverageResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Override request parameters with query parameters if provided
    if let Some(include_suggestions) = params.include_suggestions {
        request.include_suggestions = include_suggestions;
    }
    
    if let Some(schema_version) = params.schema_version {
        request.schema_version = Some(schema_version);
    }

    // Validate request
    if let Err(validation_error) = validate_analyze_coverage_request(&request) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Invalid request",
                "details": validation_error
            })),
        ));
    }

    match use_case.execute(request).await {
        Ok(response) => Ok(Json(response)),
        Err(IamError::ValidationFailed(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Validation failed",
                "details": msg
            })),
        )),
        Err(IamError::PolicyNotFound(id)) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Policy not found",
                "policy_id": id
            })),
        )),
        Err(IamError::DatabaseError(msg)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "Database error",
                "details": msg
            })),
        )),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "Internal server error",
                "details": err.to_string()
            })),
        )),
    }
}

fn validate_analyze_coverage_request(request: &AnalyzeCoverageRequest) -> Result<(), String> {
    // Validate policy IDs if provided
    if !request.policies.is_empty() {
        for policy_id in &request.policies {
            if policy_id.to_string().is_empty() {
                return Err("Policy ID cannot be empty".to_string());
            }
        }
    }

    // Validate schema version format if provided
    if let Some(ref version) = request.schema_version {
        if version.is_empty() {
            return Err("Schema version cannot be empty".to_string());
        }
        
        // Basic semantic versioning validation
        if !version.chars().any(|c| c.is_ascii_digit()) {
            return Err("Schema version must contain at least one digit".to_string());
        }
    }

    Ok(())
}

pub fn create_analyze_coverage_routes() -> Router<Arc<dyn AnalyzePolicyCoverageUseCasePort>> {
    Router::new()
        .route("/analyze-coverage", post(analyze_policy_coverage_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::PolicyId;

    #[test]
    fn test_validate_analyze_coverage_request_valid() {
        let request = AnalyzeCoverageRequest {
            policies: vec![PolicyId::new()],
            schema_version: Some("1.0.0".to_string()),
            include_suggestions: true,
        };

        assert!(validate_analyze_coverage_request(&request).is_ok());
    }

    #[test]
    fn test_validate_analyze_coverage_request_empty_policies() {
        let request = AnalyzeCoverageRequest {
            policies: vec![],
            schema_version: Some("1.0.0".to_string()),
            include_suggestions: true,
        };

        assert!(validate_analyze_coverage_request(&request).is_ok());
    }

    #[test]
    fn test_validate_analyze_coverage_request_empty_schema_version() {
        let request = AnalyzeCoverageRequest {
            policies: vec![PolicyId::new()],
            schema_version: Some("".to_string()),
            include_suggestions: true,
        };

        assert!(validate_analyze_coverage_request(&request).is_err());
    }

    #[test]
    fn test_validate_analyze_coverage_request_invalid_schema_version() {
        let request = AnalyzeCoverageRequest {
            policies: vec![PolicyId::new()],
            schema_version: Some("invalid".to_string()),
            include_suggestions: true,
        };

        assert!(validate_analyze_coverage_request(&request).is_err());
    }
}