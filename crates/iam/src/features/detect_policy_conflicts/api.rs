// crates/iam/src/features/detect_policy_conflicts/api.rs

use crate::infrastructure::errors::IamError;
use super::dto::{DetectPolicyConflictsRequest, DetectPolicyConflictsResponse};
use super::ports::PolicyConflictDetectionService;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use std::sync::Arc;
use tracing::{info, warn, error};
use serde_json::json;

/// API handler for policy conflict detection
pub struct DetectPolicyConflictsApi {
    conflict_detection_service: Arc<dyn PolicyConflictDetectionService>,
}

impl DetectPolicyConflictsApi {
    pub fn new(conflict_detection_service: Arc<dyn PolicyConflictDetectionService>) -> Self {
        Self {
            conflict_detection_service,
        }
    }

    /// Create router for this API
    pub fn router(conflict_detection_service: Arc<dyn PolicyConflictDetectionService>) -> Router {
        Router::new()
            .route("/detect-conflicts", post(detect_conflicts_handler))
            .with_state(conflict_detection_service)
    }
}

/// Handler for POST /policies/detect-conflicts
pub async fn detect_conflicts_handler(
    State(conflict_detection_service): State<Arc<dyn PolicyConflictDetectionService>>,
    Json(request): Json<DetectPolicyConflictsRequest>,
) -> Result<Json<DetectPolicyConflictsResponse>, (StatusCode, Json<serde_json::Value>)> {
    info!("Received policy conflict detection request for {} policies", request.policies.len());

    // Validate request
    if request.policies.is_empty() {
        warn!("Empty policies list in conflict detection request");
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "validation_error",
                "message": "At least one policy is required for conflict detection",
                "details": {
                    "field": "policies",
                    "reason": "required"
                }
            })),
        ));
    }

    // Check policies limit
    const MAX_POLICIES: usize = 1000;
    if request.policies.len() > MAX_POLICIES {
        warn!("Too many policies in conflict detection request: {}", request.policies.len());
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "validation_error",
                "message": "Too many policies for conflict detection",
                "details": {
                    "field": "policies",
                    "max_policies": MAX_POLICIES,
                    "actual_policies": request.policies.len()
                }
            })),
        ));
    }

    // Validate individual policies
    for (index, policy) in request.policies.iter().enumerate() {
        if policy.id.trim().is_empty() {
            warn!("Empty policy ID at index {}", index);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "validation_error",
                    "message": "Policy ID cannot be empty",
                    "details": {
                        "field": "policies",
                        "index": index,
                        "reason": "empty_id"
                    }
                })),
            ));
        }

        if policy.content.trim().is_empty() {
            warn!("Empty policy content for policy ID: {}", policy.id);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "validation_error",
                    "message": "Policy content cannot be empty",
                    "details": {
                        "field": "policies",
                        "policy_id": policy.id,
                        "reason": "empty_content"
                    }
                })),
            ));
        }

        // Check policy content size limit (e.g., 100KB per policy)
        const MAX_POLICY_SIZE: usize = 100 * 1024; // 100KB
        if policy.content.len() > MAX_POLICY_SIZE {
            warn!("Policy content too large for policy ID {}: {} bytes", policy.id, policy.content.len());
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "validation_error",
                    "message": "Policy content is too large",
                    "details": {
                        "field": "policies",
                        "policy_id": policy.id,
                        "max_size_bytes": MAX_POLICY_SIZE,
                        "actual_size_bytes": policy.content.len()
                    }
                })),
            ));
        }
    }

    // Check for duplicate policy IDs
    let mut policy_ids = std::collections::HashSet::new();
    for policy in &request.policies {
        if !policy_ids.insert(&policy.id) {
            warn!("Duplicate policy ID found: {}", policy.id);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "validation_error",
                    "message": "Duplicate policy ID found",
                    "details": {
                        "field": "policies",
                        "duplicate_id": policy.id,
                        "reason": "duplicate_id"
                    }
                })),
            ));
        }
    }

    // Perform conflict detection
    match conflict_detection_service.detect_conflicts(request).await {
        Ok(response) => {
            if response.has_conflicts {
                info!(
                    "Conflict detection completed with conflicts: {}",
                    response.get_conflict_summary()
                );
            } else {
                info!("Conflict detection completed with no conflicts found");
            }
            Ok(Json(response))
        }
        Err(e) => {
            error!("Policy conflict detection error: {}", e);
            
            let (status_code, error_response) = match &e {
                IamError::ValidationError(_) => (
                    StatusCode::BAD_REQUEST,
                    json!({
                        "error": "validation_error",
                        "message": e.to_string(),
                        "details": {
                            "type": "conflict_detection_failure"
                        }
                    }),
                ),
                IamError::ConfigurationError(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({
                        "error": "configuration_error",
                        "message": "Conflict detection service configuration error",
                        "details": {
                            "type": "service_configuration"
                        }
                    }),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({
                        "error": "internal_error",
                        "message": "An unexpected error occurred during conflict detection",
                        "details": {
                            "type": "unexpected_error"
                        }
                    }),
                ),
            };

            Err((status_code, Json(error_response)))
        }
    }
}

/// Health check handler for conflict detection service
pub async fn health_check_handler(
    State(conflict_detection_service): State<Arc<dyn PolicyConflictDetectionService>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Perform a simple conflict detection to check service health
    let test_policies = vec![
        super::dto::PolicyForAnalysis::new(
            "test-policy-1".to_string(),
            "permit(principal, action, resource);".to_string()
        ),
        super::dto::PolicyForAnalysis::new(
            "test-policy-2".to_string(),
            "permit(principal, action, resource);".to_string()
        ),
    ];
    
    let test_request = DetectPolicyConflictsRequest::new(test_policies);
    
    match conflict_detection_service.detect_conflicts(test_request).await {
        Ok(_) => Ok(Json(json!({
            "status": "healthy",
            "service": "policy_conflict_detection",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))),
        Err(e) => {
            error!("Conflict detection service health check failed: {}", e);
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "unhealthy",
                    "service": "policy_conflict_detection",
                    "error": e.to_string(),
                    "timestamp": chrono::Utc::now().to_rfc3339()
                })),
            ))
        }
    }
}

/// Create complete router with all conflict detection endpoints
pub fn create_conflict_detection_router(conflict_detection_service: Arc<dyn PolicyConflictDetectionService>) -> Router {
    Router::new()
        .route("/detect-conflicts", post(detect_conflicts_handler))
        .route("/health", axum::routing::get(health_check_handler))
        .with_state(conflict_detection_service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::dto::*;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    // Mock conflict detection service for testing
    struct MockConflictDetectionService {
        should_succeed: bool,
        should_find_conflicts: bool,
    }

    #[async_trait]
    impl PolicyConflictDetectionService for MockConflictDetectionService {
        async fn detect_conflicts(&self, request: DetectPolicyConflictsRequest) -> Result<DetectPolicyConflictsResponse, IamError> {
            if !self.should_succeed {
                return Err(IamError::validation_error("Mock conflict detection error"));
            }

            let metrics = ConflictAnalysisMetrics::default();

            if self.should_find_conflicts {
                let conflict = PolicyConflict {
                    conflict_type: ConflictType::DirectContradiction,
                    involved_policies: vec![
                        PolicyReference::new("policy1".to_string()),
                        PolicyReference::new("policy2".to_string()),
                    ],
                    description: "Mock conflict detected".to_string(),
                    severity: ConflictSeverity::High,
                    suggested_resolution: Some("Mock resolution".to_string()),
                    location: None,
                };

                let conflict_analysis = PolicyConflictAnalysis {
                    conflicts: vec![conflict],
                    redundancies: vec![],
                    unreachable_policies: vec![],
                    summary: ConflictSummary {
                        total_policies: request.policies.len(),
                        total_conflicts: 1,
                        total_redundancies: 0,
                        total_unreachable: 0,
                        conflict_score: 0.5,
                    },
                };

                Ok(DetectPolicyConflictsResponse::with_conflicts(conflict_analysis, metrics))
            } else {
                Ok(DetectPolicyConflictsResponse::no_conflicts(metrics))
            }
        }
    }

    #[tokio::test]
    async fn test_detect_conflicts_success() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        let request_body = DetectPolicyConflictsRequest::new(policies);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_detect_conflicts_with_conflicts_found() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: true 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        let request_body = DetectPolicyConflictsRequest::new(policies);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_detect_conflicts_empty_policies() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let request_body = DetectPolicyConflictsRequest::new(vec![]);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_detect_conflicts_too_many_policies() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        // Create more than 1000 policies
        let policies: Vec<PolicyForAnalysis> = (0..1001)
            .map(|i| PolicyForAnalysis::new(format!("policy{}", i), "permit(principal, action, resource);".to_string()))
            .collect();

        let request_body = DetectPolicyConflictsRequest::new(policies);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_detect_conflicts_duplicate_policy_ids() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let policies = vec![
            PolicyForAnalysis::new("duplicate-id".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("duplicate-id".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        let request_body = DetectPolicyConflictsRequest::new(policies);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_detect_conflicts_service_error() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: false, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        let request_body = DetectPolicyConflictsRequest::new(policies);

        let request = Request::builder()
            .method("POST")
            .uri("/detect-conflicts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&request_body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: true, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_check_failure() {
        let conflict_detection_service = Arc::new(MockConflictDetectionService { 
            should_succeed: false, 
            should_find_conflicts: false 
        });
        let app = create_conflict_detection_router(conflict_detection_service);

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}