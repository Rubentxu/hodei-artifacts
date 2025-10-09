//! Policy validation and evaluation handlers
//!
//! This module provides HTTP handlers for policy-related operations:
//! - Validating Cedar policies (syntax and schema checking)
//! - Evaluating policies against authorization requests

use crate::app_state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use serde::{Deserialize, Serialize};

/// Request to validate a policy
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ValidatePolicyRequest {
    /// Cedar policy content to validate
    pub content: String,
    /// Whether to use schema-based validation
    #[serde(default = "default_use_schema")]
    pub use_schema: bool,
}

fn default_use_schema() -> bool {
    true
}

/// Response from policy validation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ValidatePolicyResponse {
    /// Whether the policy is valid
    pub is_valid: bool,
    /// Validation errors (if any)
    pub errors: Vec<String>,
}

/// Request to evaluate policies
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EvaluatePoliciesRequest {
    /// Principal HRN (e.g., "hrn:aws:iam::123:user/alice")
    pub principal_hrn: String,
    /// Action being performed (e.g., "CreateUser")
    pub action: String,
    /// Resource HRN (e.g., "hrn:aws:iam::123:user/bob")
    pub resource_hrn: String,
    /// Cedar policy content (inline policies)
    pub policies: Vec<String>,
    /// Optional context for the evaluation
    #[serde(default)]
    pub context: serde_json::Value,
    /// Optional schema version to use
    pub schema_version: Option<String>,
    /// Evaluation mode
    #[serde(default)]
    pub evaluation_mode: String,
}

/// Response from policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EvaluatePoliciesResponse {
    /// Decision: "Allow" or "Deny"
    pub decision: String,
    /// Policies that determined the decision
    pub determining_policies: Vec<String>,
    /// Reasons for the decision
    pub reasons: Vec<String>,
    /// Schema version used (if any)
    pub used_schema_version: Option<String>,
    /// Policy IDs that were evaluated
    pub policy_ids_evaluated: Vec<String>,
    /// Diagnostic information
    pub diagnostics: Vec<DiagnosticInfo>,
}

/// Diagnostic information from policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DiagnosticInfo {
    /// Diagnostic level: "info", "warning", "error"
    pub level: String,
    /// Diagnostic message
    pub message: String,
    /// Optional policy ID related to this diagnostic
    pub policy_id: Option<String>,
}

/// Handler to validate a policy
///
/// This endpoint validates a Cedar policy for syntax correctness
/// and optionally checks it against the active schema.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy validation request
///
/// # Returns
///
/// A JSON response with validation results or an error
///
/// # Example Request
///
/// ```json
/// {
///   "content": "permit(principal, action, resource);",
///   "use_schema": true
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "is_valid": true,
///   "errors": [],
///   "warnings": []
/// }
/// ```
#[utoipa::path(
    post,
    path = "/api/v1/policies/validate",
    tag = "policies",
    request_body = ValidatePolicyRequest,
    responses(
        (status = 200, description = "Policy validated successfully", body = ValidatePolicyResponse),
        (status = 400, description = "Invalid policy content"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn validate_policy<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<ValidatePolicyRequest>,
) -> Result<Json<ValidatePolicyResponse>, ApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let command = hodei_policies::features::validate_policy::dto::ValidatePolicyCommand {
        content: request.content,
    };

    let result =
        state.validate_policy.execute(command).await.map_err(|e| {
            ApiError::InternalServerError(format!("Failed to validate policy: {}", e))
        })?;

    Ok(Json(ValidatePolicyResponse {
        is_valid: result.is_valid,
        errors: result.errors,
        // Note: ValidationResult from hodei-policies doesn't include warnings field
    }))
}

/// Handler to evaluate policies
///
/// This endpoint evaluates an authorization request against a set of policies.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy evaluation request
///
/// # Returns
///
/// A JSON response with the evaluation decision or an error
///
/// # Example Request
///
/// ```json
/// {
///   "principal_hrn": "hrn:aws:iam::123:user/alice",
///   "action": "CreateUser",
///   "resource_hrn": "hrn:aws:iam::123:user/bob",
///   "policies": [
///     "permit(principal, action, resource);"
///   ],
///   "context": {},
///   "evaluation_mode": "BestEffortNoSchema"
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "decision": "Allow",
///   "determining_policies": [],
///   "reasons": [],
///   "used_schema_version": null,
///   "policy_ids_evaluated": ["policy_0"],
///   "diagnostics": []
/// }
/// ```
#[utoipa::path(
    post,
    path = "/api/v1/policies/evaluate",
    tag = "policies",
    request_body = EvaluatePoliciesRequest,
    responses(
        (status = 200, description = "Policies evaluated successfully", body = EvaluatePoliciesResponse),
        (status = 400, description = "Invalid evaluation request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn evaluate_policies<S>(
    State(_state): State<AppState<S>>,
    Json(_request): Json<EvaluatePoliciesRequest>,
) -> Result<Json<EvaluatePoliciesResponse>, ApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy evaluation
    // This requires:
    // 1. Parsing principal_hrn and resource_hrn into entities
    // 2. Creating HodeiPolicy instances from the policy strings
    // 3. Building the EvaluatePoliciesCommand
    // 4. Calling the use case
    // 5. Mapping the result to the response

    // For now, return a stub response
    Ok(Json(EvaluatePoliciesResponse {
        decision: "Deny".to_string(),
        determining_policies: vec![],
        reasons: vec!["Evaluation not yet implemented".to_string()],
        used_schema_version: None,
        policy_ids_evaluated: vec![],
        diagnostics: vec![DiagnosticInfo {
            level: "warning".to_string(),
            message: "Policy evaluation endpoint is a stub".to_string(),
            policy_id: None,
        }],
    }))
}

/// API Error type for handler responses
#[derive(Debug)]
pub enum ApiError {
    InternalServerError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_policy_request_default_use_schema() {
        let json = r#"{"content": "permit(principal, action, resource);"}"#;
        let request: ValidatePolicyRequest = serde_json::from_str(json).unwrap();
        assert!(request.use_schema);
    }

    #[test]
    fn test_evaluate_policies_request_serialization() {
        let request = EvaluatePoliciesRequest {
            principal_hrn: "hrn:aws:iam::123:user/alice".to_string(),
            action: "CreateUser".to_string(),
            resource_hrn: "hrn:aws:iam::123:user/bob".to_string(),
            policies: vec!["permit(principal, action, resource);".to_string()],
            context: serde_json::json!({}),
            schema_version: None,
            evaluation_mode: "NoSchema".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("alice"));
        assert!(json.contains("CreateUser"));
    }

    #[test]
    fn test_validate_policy_response_serialization() {
        let response = ValidatePolicyResponse {
            is_valid: true,
            errors: vec![],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("true"));
    }
}
