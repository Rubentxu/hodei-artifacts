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

use hodei_policies::evaluate_policies::dto::{
    AuthorizationRequest, Decision, DiagnosticLevel, EvaluatePoliciesCommand, EvaluationMode,
};
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
use kernel::{AttributeName, AttributeValue, HodeiEntity, Hrn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
pub async fn validate_policy(
    State(state): State<AppState>,
    Json(request): Json<ValidatePolicyRequest>,
) -> Result<Json<ValidatePolicyResponse>, ApiError> {
    let command = hodei_policies::validate_policy::dto::ValidatePolicyCommand {
        content: request.content,
    };

    let result =
        state.validate_policy.validate(command).await.map_err(|e| {
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
/// The evaluation is real: the request is translated to the internal
/// authorization model and evaluated against the supplied policies. The
/// decision follows Cedar's rules, so an explicit `forbid` prevails over a
/// `permit` and the default is deny.
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
///   "determining_policies": ["policy_0"],
///   "reasons": ["Access granted"],
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
pub async fn evaluate_policies(
    State(state): State<AppState>,
    Json(request): Json<EvaluatePoliciesRequest>,
) -> Result<Json<EvaluatePoliciesResponse>, ApiError> {
    // 1. Parse the HRNs of the request
    let principal_hrn = Hrn::from_string(&request.principal_hrn).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "Invalid principal HRN: '{}'",
            request.principal_hrn
        ))
    })?;

    let resource_hrn = Hrn::from_string(&request.resource_hrn).ok_or_else(|| {
        ApiError::BadRequest(format!("Invalid resource HRN: '{}'", request.resource_hrn))
    })?;

    if request.action.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "The action must not be empty".to_string(),
        ));
    }

    // 2. Parse the evaluation mode and the context
    let evaluation_mode = parse_evaluation_mode(&request.evaluation_mode)?;
    let context = parse_context(request.context)?;

    // 3. Build the policy set; the inline policies are identified by their
    //    position, so the decision can point at the one that determined it.
    let policy_set = build_policy_set(&request.policies);

    // 4. Build the entities of the request
    let principal = HrnEntity::new(principal_hrn);
    let resource = HrnEntity::new(resource_hrn);
    let entities: [&dyn HodeiEntity; 2] = [&principal, &resource];

    // 5. Evaluate
    let command = EvaluatePoliciesCommand {
        request: AuthorizationRequest {
            principal: &principal,
            action: &request.action,
            resource: &resource,
            context: Some(context),
        },
        policies: &policy_set,
        entities: &entities,
        schema_version: request.schema_version,
        evaluation_mode,
    };

    let result = state
        .evaluate_policies
        .evaluate(command)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("Policy evaluation failed: {}", e)))?;

    // 6. Map the result to the response
    let decision = match result.decision {
        Decision::Allow => "Allow",
        Decision::Deny => "Deny",
    };

    Ok(Json(EvaluatePoliciesResponse {
        decision: decision.to_string(),
        determining_policies: result.determining_policies,
        reasons: result.reasons,
        used_schema_version: result.used_schema_version,
        policy_ids_evaluated: result.policy_ids_evaluated,
        diagnostics: result
            .diagnostics
            .iter()
            .map(|diagnostic| DiagnosticInfo {
                level: diagnostic_level_name(&diagnostic.level).to_string(),
                message: diagnostic.message.clone(),
                policy_id: diagnostic.policy_id.clone(),
            })
            .collect(),
    }))
}

/// An entity built from an HRN supplied by the caller
///
/// The endpoint receives identities as HRN strings, so the entities it
/// evaluates carry no attributes: their identity is the HRN itself, which
/// includes the whole scope (partition, service, region, account, type and
/// id).
#[derive(Debug)]
struct HrnEntity {
    hrn: Hrn,
}

impl HrnEntity {
    fn new(hrn: Hrn) -> Self {
        Self { hrn }
    }
}

impl HodeiEntity for HrnEntity {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        HashMap::new()
    }
}

/// Build the policy set from the inline policies of the request
///
/// The identifier of each policy is its position in the request, so the
/// decision can name the policy that determined it.
fn build_policy_set(policies: &[String]) -> HodeiPolicySet {
    let policies = policies
        .iter()
        .enumerate()
        .map(|(index, content)| {
            HodeiPolicy::new(PolicyId::new(format!("policy_{}", index)), content.clone())
        })
        .collect();

    HodeiPolicySet::new(policies)
}

/// Parse the evaluation mode of the request
///
/// An absent mode means the default: best effort without schema.
fn parse_evaluation_mode(mode: &str) -> Result<EvaluationMode, ApiError> {
    match mode.trim() {
        "" => Ok(EvaluationMode::BestEffortNoSchema),
        "Strict" => Ok(EvaluationMode::Strict),
        "BestEffortNoSchema" => Ok(EvaluationMode::BestEffortNoSchema),
        "NoSchema" => Ok(EvaluationMode::NoSchema),
        other => Err(ApiError::BadRequest(format!(
            "Unknown evaluation mode: '{}'. Valid modes are Strict, BestEffortNoSchema and NoSchema",
            other
        ))),
    }
}

/// Parse the context of the request
///
/// The context must be a JSON object; anything else is a malformed request.
fn parse_context(
    context: serde_json::Value,
) -> Result<HashMap<String, serde_json::Value>, ApiError> {
    match context {
        serde_json::Value::Null => Ok(HashMap::new()),
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        other => Err(ApiError::BadRequest(format!(
            "The context must be a JSON object, got: {}",
            other
        ))),
    }
}

/// The wire name of a diagnostic level
fn diagnostic_level_name(level: &DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "info",
        DiagnosticLevel::Warning => "warning",
        DiagnosticLevel::Error => "error",
    }
}

/// API Error type for handler responses
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    InternalServerError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
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
