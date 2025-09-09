// crates/iam/src/features/validate_policy/api_test.rs

#[cfg(test)]
mod tests {
    use super::super::api::*;
    use super::super::dto::*;
    use super::super::ports::*;
    use crate::infrastructure::errors::IamError;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    // Mock validation service for API testing
    struct MockValidationService {
        should_succeed: bool,
        should_be_valid: bool,
        should_timeout: bool,
    }

    #[async_trait]
    impl PolicyValidationService for MockValidationService {
        async fn validate_policy(&self, command: ValidatePolicyCommand) -> Result<ValidatePolicyResponse, IamError> {
            if self.should_timeout {
                return Err(IamError::validation_error("Validation timeout"));
            }

            if !self.should_succeed {
                return Err(IamError::validation_error("Mock validation service error"));
            }

            let validation_result = if self.should_be_valid {
                PolicyValidationResult {
                    syntax_errors: vec![],
                    semantic_errors: vec![],
                    hrn_errors: vec![],
                    warnings: vec![],
                    cross_policy_issues: vec![],
                    schema_info: Some(SchemaValidationInfo {
                        version: "v1.0".to_string(),
                        entities_validated: vec!["User".to_string()],
                        actions_validated: vec!["read".to_string()],
                    }),
                }
            } else {
                PolicyValidationResult {
                    syntax_errors: vec![ValidationError {
                        error_type: ValidationErrorType::SyntaxError,
                        message: "Mock syntax error".to_string(),
                        location: Some(PolicyLocation {
                            line: 1,
                            column: 10,
                            offset: Some(9),
                        }),
                        suggested_fix: Some("Fix the syntax".to_string()),
                        documentation_link: Some("https://docs.example.com".to_string()),
                    }],
                    semantic_errors: vec![],
                    hrn_errors: vec![],
                    warnings: vec![],
                    cross_policy_issues: vec![],
                    schema_info: None,
                }
            };

            Ok(ValidatePolicyResponse {
                is_valid: self.should_be_valid,
                validation_result,
                metrics: ValidationMetrics {
                    validation_time_ms: 100,
                    syntax_validation_time_ms: 30,
                    semantic_validation_time_ms: 50,
                    hrn_validation_time_ms: 20,
                    memory_usage_bytes: Some(1024),
                },
                validation_id: "test-validation-123".to_string(),
            })
        }

        async fn validate_policies_batch(&self, command: ValidatePoliciesBatchCommand) -> Result<ValidatePoliciesBatchResponse, IamError> {
            if self.should_timeout {
                return Err(IamError::validation_error("Batch validation timeout"));
            }

            if !self.should_succeed {
                return Err(IamError::validation_error("Mock batch validation service error"));
            }

            let individual_results: Vec<IndividualValidationResult> = command.policies
                .iter()
                .enumerate()
                .map(|(index, policy)| IndividualValidationResult {
                    index,
                    policy_id: policy.id.clone(),
                    is_valid: self.should_be_valid,
                    validation_result: PolicyValidationResult {
                        syntax_errors: if self.should_be_valid { vec![] } else {
                            vec![ValidationError {
                                error_type: ValidationErrorType::SyntaxError,
                                message: format!("Mock error for policy {}", index),
                                location: None,
                                suggested_fix: None,
                                documentation_link: None,
                            }]
                        },
                        semantic_errors: vec![],
                        hrn_errors: vec![],
                        warnings: vec![],
                        cross_policy_issues: vec![],
                        schema_info: None,
                    },
                })
                .collect();

            let batch_metrics = BatchValidationMetrics {
                total_time_ms: 500,
                individual_validation_time_ms: 400,
                cross_policy_validation_time_ms: 100,
                total_policies: command.policies.len(),
                valid_policies: if self.should_be_valid { command.policies.len() } else { 0 },
                invalid_policies: if self.should_be_valid { 0 } else { command.policies.len() },
            };

            Ok(ValidatePoliciesBatchResponse {
                overall_valid: self.should_be_valid,
                individual_results,
                cross_policy_validation: Some(CrossPolicyValidationResult {
                    conflicts: vec![],
                    redundancies: vec![],
                    coverage_gaps: vec![],
                }),
                batch_metrics,
                batch_id: "test-batch-456".to_string(),
            })
        }
    }

    fn create_test_app(mock_service: MockValidationService) -> axum::Router {
        ValidatePolicyApi::router(Arc::new(mock_service))
    }

    #[tokio::test]
    async fn test_validate_policy_api_success() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let validation_response: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
        
        assert!(validation_response.is_valid);
        assert_eq!(validation_response.validation_id, "test-validation-123");
        assert!(validation_response.validation_result.syntax_errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_policy_api_invalid_policy() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: false,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "invalid syntax".to_string(),
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let validation_response: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
        
        assert!(!validation_response.is_valid);
        assert!(!validation_response.validation_result.syntax_errors.is_empty());
        assert_eq!(validation_response.validation_result.syntax_errors[0].message, "Mock syntax error");
    }

    #[tokio::test]
    async fn test_validate_policy_api_empty_content() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "".to_string(),
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "validation_error");
        assert!(error_response["message"].as_str().unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_validate_policy_api_missing_requested_by() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "validation_error");
        assert!(error_response["message"].as_str().unwrap().contains("requested_by"));
    }

    #[tokio::test]
    async fn test_validate_policy_api_large_content() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        // Create content larger than typical limit (e.g., 1MB)
        let large_content = "permit(principal, action, resource);".repeat(50000);
        let command = ValidatePolicyCommand {
            content: large_content,
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        // Should either accept large content or reject with appropriate error
        match response.status() {
            StatusCode::OK => {
                // If accepted, should process successfully
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let validation_response: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
                assert!(!validation_response.validation_id.is_empty());
            }
            StatusCode::BAD_REQUEST => {
                // If rejected, should have appropriate error message
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert!(error_response["message"].as_str().unwrap().contains("too large") ||
                       error_response["message"].as_str().unwrap().contains("size"));
            }
            _ => panic!("Unexpected status code for large content: {}", response.status()),
        }
    }

    #[tokio::test]
    async fn test_validate_policy_api_service_error() {
        let app = create_test_app(MockValidationService {
            should_succeed: false,
            should_be_valid: true,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "internal_error");
        assert!(error_response["message"].as_str().unwrap().contains("unexpected error"));
    }

    #[tokio::test]
    async fn test_validate_policy_api_timeout() {
        let app = create_test_app(MockValidationService {
            should_succeed: false,
            should_be_valid: true,
            should_timeout: true,
        });

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: Some(ValidationOptions {
                include_warnings: Some(true),
                deep_validation: Some(true),
                schema_version: None,
                timeout_ms: Some(1), // Very short timeout
            }),
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "validation_error");
        assert!(error_response["message"].as_str().unwrap().contains("timeout"));
    }

    #[tokio::test]
    async fn test_validate_policy_api_invalid_json() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from("invalid json content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_validate_policies_batch_api_success() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let policies = vec![
            PolicyToValidate {
                id: Some("policy-1".to_string()),
                content: "permit(principal, action, resource);".to_string(),
                metadata: None,
            },
            PolicyToValidate {
                id: Some("policy-2".to_string()),
                content: "forbid(principal, action, resource);".to_string(),
                metadata: None,
            },
        ];

        let batch_command = ValidatePoliciesBatchCommand {
            policies,
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate-batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&batch_command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let batch_response: ValidatePoliciesBatchResponse = serde_json::from_slice(&body).unwrap();
        
        assert!(batch_response.overall_valid);
        assert_eq!(batch_response.individual_results.len(), 2);
        assert_eq!(batch_response.batch_id, "test-batch-456");
        assert_eq!(batch_response.batch_metrics.total_policies, 2);
    }

    #[tokio::test]
    async fn test_validate_policies_batch_api_empty_batch() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let batch_command = ValidatePoliciesBatchCommand {
            policies: vec![],
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate-batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&batch_command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "validation_error");
        assert!(error_response["message"].as_str().unwrap().contains("At least one policy"));
    }

    #[tokio::test]
    async fn test_validate_policies_batch_api_too_many_policies() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        // Create more than the typical limit (e.g., 1000 policies)
        let mut policies = Vec::new();
        for i in 0..1001 {
            policies.push(PolicyToValidate {
                id: Some(format!("policy-{}", i)),
                content: "permit(principal, action, resource);".to_string(),
                metadata: None,
            });
        }

        let batch_command = ValidatePoliciesBatchCommand {
            policies,
            options: None,
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate-batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&batch_command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(error_response["error"], "validation_error");
        assert!(error_response["message"].as_str().unwrap().contains("Too many policies"));
    }

    #[tokio::test]
    async fn test_validate_policy_api_with_options() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let options = ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: Some("v2.0".to_string()),
            timeout_ms: Some(10000),
        };

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource) when context.authenticated == true;".to_string(),
            options: Some(options),
            requested_by: "test_user".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let validation_response: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
        
        assert!(validation_response.is_valid);
        assert!(!validation_response.validation_id.is_empty());
        // Verify that options were processed (schema info should be present)
        assert!(validation_response.validation_result.schema_info.is_some());
    }

    #[tokio::test]
    async fn test_validate_policy_api_health_check() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let health_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(health_response["status"], "healthy");
        assert_eq!(health_response["service"], "policy_validation");
        assert!(health_response["timestamp"].is_string());
    }

    #[tokio::test]
    async fn test_validate_policy_api_health_check_unhealthy() {
        let app = create_test_app(MockValidationService {
            should_succeed: false,
            should_be_valid: true,
            should_timeout: false,
        });

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let health_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(health_response["status"], "unhealthy");
        assert_eq!(health_response["service"], "policy_validation");
        assert!(health_response["error"].is_string());
    }

    #[tokio::test]
    async fn test_validate_policy_api_cors_headers() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let request = Request::builder()
            .method("OPTIONS")
            .uri("/validate")
            .header("Origin", "https://example.com")
            .header("Access-Control-Request-Method", "POST")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        // Should handle CORS preflight requests appropriately
        // The exact status depends on CORS configuration
        assert!(response.status() == StatusCode::OK || 
               response.status() == StatusCode::NO_CONTENT ||
               response.status() == StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_validate_policy_api_content_type_validation() {
        let app = create_test_app(MockValidationService {
            should_succeed: true,
            should_be_valid: true,
            should_timeout: false,
        });

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "test_user".to_string(),
        };

        // Test with wrong content type
        let request = Request::builder()
            .method("POST")
            .uri("/validate")
            .header("content-type", "text/plain")
            .body(Body::from(serde_json::to_string(&command).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        // Should either accept it (if content-type is not strictly enforced)
        // or reject with appropriate error
        match response.status() {
            StatusCode::OK => {
                // If accepted, should process successfully
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let validation_response: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
                assert!(!validation_response.validation_id.is_empty());
            }
            StatusCode::BAD_REQUEST | StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                // If rejected, should have appropriate error
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let error_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert!(error_response["error"].is_string());
            }
            _ => panic!("Unexpected status code for wrong content type: {}", response.status()),
        }
    }
}