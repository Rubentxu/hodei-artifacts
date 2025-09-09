// crates/iam/src/features/validate_policy/api.rs

use crate::features::validate_policy::dto::*;
use crate::features::validate_policy::ports::PolicyValidationService;
use crate::infrastructure::errors::IamError;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{post},
    Router,
};
use std::sync::Arc;
use tracing::{info, warn};

/// API handlers for policy validation endpoints
pub struct ValidatePolicyApi {
    validation_service: Arc<dyn PolicyValidationService>,
}

impl ValidatePolicyApi {
    pub fn new(validation_service: Arc<dyn PolicyValidationService>) -> Self {
        Self { validation_service }
    }

    /// Create router with all validation endpoints
    pub fn create_router(validation_service: Arc<dyn PolicyValidationService>) -> Router {
        let api = Arc::new(Self::new(validation_service.clone()));

        Router::new()
            .route("/validate", post(validate_policy_handler))
            .route("/validate/batch", post(validate_policies_batch_handler))
            .with_state(api)
    }
}

/// Handler for single policy validation
/// POST /validate
async fn validate_policy_handler(
    State(api): State<Arc<ValidatePolicyApi>>,
    Json(command): Json<ValidatePolicyCommand>,
) -> Result<Json<ValidatePolicyResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("Received policy validation request from user: {}", command.requested_by);

    match api.validation_service.validate_policy(command).await {
        Ok(response) => {
            info!("Policy validation completed successfully, valid: {}", response.is_valid);
            Ok(Json(response))
        }
        Err(e) => {
            warn!("Policy validation failed: {}", e);
            let error_response = ErrorResponse::from_iam_error(e);
            Err((error_response.status_code(), Json(error_response)))
        }
    }
}

/// Handler for batch policy validation
/// POST /validate/batch
async fn validate_policies_batch_handler(
    State(api): State<Arc<ValidatePolicyApi>>,
    Json(command): Json<ValidatePoliciesBatchCommand>,
) -> Result<Json<ValidatePoliciesBatchResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!(
        "Received batch policy validation request from user: {}, policies: {}",
        command.requested_by,
        command.policies.len()
    );

    match api.validation_service.validate_policies_batch(command).await {
        Ok(response) => {
            info!(
                "Batch policy validation completed successfully, overall_valid: {}, processed: {}",
                response.overall_valid,
                response.batch_metrics.policies_processed
            );
            Ok(Json(response))
        }
        Err(e) => {
            warn!("Batch policy validation failed: {}", e);
            let error_response = ErrorResponse::from_iam_error(e);
            Err((error_response.status_code(), Json(error_response)))
        }
    }
}

/// Standard error response format
#[derive(Debug, serde::Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: String,
}

impl ErrorResponse {
    pub fn from_iam_error(error: IamError) -> Self {
        let (error_type, message, details) = match &error {
            IamError::InvalidInput(msg) => ("INVALID_INPUT", msg.clone(), None),
            IamError::PolicyValidationFailed { errors } => (
                "VALIDATION_FAILED",
                "Policy validation failed".to_string(),
                Some(format!("Validation errors: {}", 
                    errors.iter().map(|e| &e.message).cloned().collect::<Vec<_>>().join(", ")
                )),
            ),
            IamError::ConfigurationError(msg) => ("CONFIGURATION_ERROR", msg.clone(), None),
            IamError::DatabaseError(msg) => ("DATABASE_ERROR", msg.clone(), None),
            IamError::InternalError(msg) => ("INTERNAL_ERROR", msg.clone(), None),
            _ => ("UNKNOWN_ERROR", "An unknown error occurred".to_string(), None),
        };

        Self {
            error: error_type.to_string(),
            message,
            details,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self.error.as_str() {
            "INVALID_INPUT" => StatusCode::BAD_REQUEST,
            "VALIDATION_FAILED" => StatusCode::BAD_REQUEST,
            "CONFIGURATION_ERROR" => StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR" => StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::validate_policy::dto::*;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    // Mock implementation for testing
    struct MockValidationService {
        should_succeed: bool,
    }

    #[async_trait]
    impl PolicyValidationService for MockValidationService {
        async fn validate_policy(&self, _command: ValidatePolicyCommand) -> Result<ValidatePolicyResponse, IamError> {
            if self.should_succeed {
                let validation_result = PolicyValidationResult {
                    syntax_errors: Vec::new(),
                    semantic_errors: Vec::new(),
                    hrn_errors: Vec::new(),
                    warnings: Vec::new(),
                    schema_info: SchemaValidationInfo {
                        version: "1.0.0".to_string(),
                        schema_id: "test".to_string(),
                        entity_types_count: 2,
                        actions_count: 2,
                    },
                };

                let metrics = ValidationMetrics {
                    validation_time_ms: 100,
                    memory_usage_bytes: 1024,
                    validation_steps: 3,
                    schema_load_time_ms: 10,
                };

                Ok(ValidatePolicyResponse::new(validation_result, metrics))
            } else {
                Err(IamError::InvalidInput("Test error".to_string()))
            }
        }

        async fn validate_policies_batch(&self, command: ValidatePoliciesBatchCommand) -> Result<ValidatePoliciesBatchResponse, IamError> {
            if self.should_succeed {
                let individual_results = command.policies.iter().enumerate().map(|(index, policy)| {
                    IndividualValidationResult {
                        index,
                        policy_id: policy.id.clone(),
                        is_valid: true,
                        validation_result: PolicyValidationResult {
                            syntax_errors: Vec::new(),
                            semantic_errors: Vec::new(),
                            hrn_errors: Vec::new(),
                            warnings: Vec::new(),
                            schema_info: SchemaValidationInfo {
                                version: "1.0.0".to_string(),
                                schema_id: "test".to_string(),
                                entity_types_count: 2,
                                actions_count: 2,
                            },
                        },
                    }
                }).collect();

                let batch_metrics = BatchValidationMetrics {
                    total_time_ms: 200,
                    average_time_per_policy_ms: 100,
                    policies_processed: command.policies.len(),
                    policies_passed: command.policies.len(),
                    total_memory_usage_bytes: 2048,
                };

                Ok(ValidatePoliciesBatchResponse::new(individual_results, None, batch_metrics))
            } else {
                Err(IamError::InvalidInput("Test batch error".to_string()))
            }
        }
    }

    #[tokio::test]
    async fn test_validate_policy_success() {
        let mock_service = Arc::new(MockValidationService { should_succeed: true });
        let app = ValidatePolicyApi::create_router(mock_service);

        let command = ValidatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            "test_user".to_string(),
        );

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_validate_policy_failure() {
        let mock_service = Arc::new(MockValidationService { should_succeed: false });
        let app = ValidatePolicyApi::create_router(mock_service);

        let command = ValidatePolicyCommand::new(
            "invalid policy".to_string(),
            "test_user".to_string(),
        );

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_validate_policies_batch_success() {
        let mock_service = Arc::new(MockValidationService { should_succeed: true });
        let app = ValidatePolicyApi::create_router(mock_service);

        let policies = vec![
            PolicyToValidate::new("permit(principal, action, resource);".to_string()),
            PolicyToValidate::new("forbid(principal, action, resource);".to_string()),
        ];

        let command = ValidatePoliciesBatchCommand::new(policies, "test_user".to_string());

        let request = Request::builder()
            .method("POST")
            .uri("/validate/batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}