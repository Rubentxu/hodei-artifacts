//! Playground handlers for Hodei Artifacts API
//!
//! This module provides HTTP handlers for the policy playground feature,
//! allowing ad-hoc policy evaluation and testing without persistence.

use crate::app_state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use hodei_policies::features::playground_evaluate::dto::{
    AttributeValue, PlaygroundAuthorizationRequest, PlaygroundEvaluateResult,
};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request for playground policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PlaygroundEvaluateRequest {
    /// Optional inline Cedar schema (JSON format)
    /// If None, must provide schema_version
    pub inline_schema: Option<String>,

    /// Optional reference to a stored schema version
    /// If None, must provide inline_schema
    pub schema_version: Option<String>,

    /// Inline Cedar policies to evaluate (policy text)
    /// Each string is a complete Cedar policy
    pub inline_policies: Vec<String>,

    /// The authorization request to evaluate
    pub request: PlaygroundAuthorizationRequestDto,
}

/// Authorization request DTO for playground evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PlaygroundAuthorizationRequestDto {
    /// The principal (user/service) making the request
    pub principal: String,

    /// The action being requested
    pub action: String,

    /// The resource being accessed
    pub resource: String,

    /// Optional context attributes for the request
    #[serde(default)]
    pub context: HashMap<String, AttributeValueDto>,
}

/// Attribute value DTO for context
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", content = "value")]
pub enum AttributeValueDto {
    /// String value
    String(String),
    /// Integer value
    Long(i64),
    /// Boolean value
    Bool(bool),
    /// Entity reference (HRN)
    EntityRef(String),
    /// Set of string values (simplified to avoid recursion)
    Set(Vec<String>),
    /// Record of string values (simplified to avoid recursion)
    Record(HashMap<String, String>),
}

/// Response from playground policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PlaygroundEvaluateResponse {
    /// The authorization decision (Allow/Deny)
    pub decision: String,

    /// Policies that contributed to the decision
    pub determining_policies: Vec<DeterminingPolicyDto>,

    /// Diagnostic information about the evaluation
    pub diagnostics: EvaluationDiagnosticsDto,

    /// Errors encountered during evaluation (if any)
    pub errors: Vec<String>,
}

/// Determining policy DTO
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeterminingPolicyDto {
    /// The policy ID or inline index
    pub policy_id: String,

    /// The effect of the policy (permit or forbid)
    pub effect: String,

    /// The policy text (for inline policies)
    pub policy_text: Option<String>,
}

/// Evaluation diagnostics DTO
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EvaluationDiagnosticsDto {
    /// Total number of policies evaluated
    pub total_policies: usize,

    /// Number of policies that matched
    pub matched_policies: usize,

    /// Whether schema validation was performed
    pub schema_validated: bool,

    /// Validation errors (if any)
    pub validation_errors: Vec<String>,

    /// Warnings (if any)
    pub warnings: Vec<String>,
}

/// Handler for playground policy evaluation
///
/// This endpoint allows ad-hoc evaluation of Cedar policies against
/// authorization requests in a playground environment, without requiring
/// persistence of policies or schemas.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Playground evaluation request
///
/// # Returns
///
/// A JSON response with the evaluation result or an error
///
/// # Example Request
///
/// ```json
/// {
///   "inline_schema": "{\"entityTypes\": {\"User\": {\"shape\": {\"type\": \"Record\", \"attributes\": {}}}}}",
///   "inline_policies": [
///     "permit(principal, action, resource);"
///   ],
///   "request": {
///     "principal": "hodei::iam::default::User::alice",
///     "action": "hodei::api::Action::read",
///     "resource": "hodei::storage::default::Document::doc1",
///     "context": {
///       "ip": {
///         "type": "String",
///         "value": "192.168.1.1"
///       }
///     }
///   }
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "decision": "ALLOW",
///   "determining_policies": [
///     {
///       "policy_id": "policy_0",
///       "effect": "permit",
///       "policy_text": "permit(principal, action, resource);"
///     }
///   ],
///   "diagnostics": {
///     "total_policies": 1,
///     "matched_policies": 1,
///     "schema_validated": true,
///     "validation_errors": [],
///     "warnings": []
///   },
///   "errors": []
/// }
/// ```
#[utoipa::path(
    post,
    path = "/api/v1/playground/evaluate",
    tag = "playground",
    request_body = PlaygroundEvaluateRequest,
    responses(
        (status = 200, description = "Policy evaluation completed successfully", body = PlaygroundEvaluateResponse),
        (status = 400, description = "Invalid request parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn playground_evaluate(
    State(state): State<AppState>,
    Json(request): Json<PlaygroundEvaluateRequest>,
) -> Result<Json<PlaygroundEvaluateResponse>, ApiError> {
    // Convert HTTP DTO to domain DTO
    let command = convert_to_command(request)
        .map_err(|e| ApiError::BadRequest(format!("Invalid request: {}", e)))?;

    // Execute the playground evaluation use case
    let result = state
        .playground_evaluate
        .evaluate(command)
        .await
        .map_err(|e| {
            ApiError::InternalServerError(format!("Playground evaluation failed: {}", e))
        })?;

    // Convert domain result to HTTP response
    let response = convert_to_response(result);

    Ok(Json(response))
}

/// Convert HTTP request to domain command
fn convert_to_command(
    request: PlaygroundEvaluateRequest,
) -> Result<hodei_policies::features::playground_evaluate::dto::PlaygroundEvaluateCommand, String> {
    // Convert principal, action, and resource to HRNs
    let principal = Hrn::from_string(&request.request.principal)
        .ok_or_else(|| format!("Invalid principal HRN: {}", &request.request.principal))?;

    let action = Hrn::from_string(&request.request.action)
        .ok_or_else(|| format!("Invalid action HRN: {}", &request.request.action))?;

    let resource = Hrn::from_string(&request.request.resource)
        .ok_or_else(|| format!("Invalid resource HRN: {}", &request.request.resource))?;

    // Convert context attributes
    let mut context = HashMap::new();
    for (key, value) in request.request.context {
        let converted_value = convert_attribute_value(value)?;
        context.insert(key, converted_value);
    }

    // Create authorization request
    let auth_request = PlaygroundAuthorizationRequest {
        principal,
        action,
        resource,
        context,
    };

    // Create command
    let command = hodei_policies::features::playground_evaluate::dto::PlaygroundEvaluateCommand {
        inline_schema: request.inline_schema,
        schema_version: request.schema_version,
        inline_policies: request.inline_policies,
        request: auth_request,
    };

    Ok(command)
}

/// Convert attribute value DTO to domain attribute value
fn convert_attribute_value(dto: AttributeValueDto) -> Result<AttributeValue, String> {
    match dto {
        AttributeValueDto::String(s) => Ok(AttributeValue::String(s)),
        AttributeValueDto::Long(n) => Ok(AttributeValue::Long(n)),
        AttributeValueDto::Bool(b) => Ok(AttributeValue::Bool(b)),
        AttributeValueDto::EntityRef(hrn_str) => {
            let hrn = Hrn::from_string(&hrn_str)
                .ok_or_else(|| format!("Invalid entity reference HRN: {}", &hrn_str))?;
            Ok(AttributeValue::EntityRef(hrn))
        }
        AttributeValueDto::Set(values) => {
            let converted_values: Vec<AttributeValue> = values
                .into_iter()
                .map(|s| AttributeValue::String(s))
                .collect();
            Ok(AttributeValue::Set(converted_values))
        }
        AttributeValueDto::Record(record) => {
            let mut converted_record = HashMap::new();
            for (key, value) in record {
                converted_record.insert(key, AttributeValue::String(value));
            }
            Ok(AttributeValue::Record(converted_record))
        }
    }
}

/// Convert domain result to HTTP response
fn convert_to_response(result: PlaygroundEvaluateResult) -> PlaygroundEvaluateResponse {
    // Convert determining policies
    let determining_policies: Vec<DeterminingPolicyDto> = result
        .determining_policies
        .into_iter()
        .map(|policy| DeterminingPolicyDto {
            policy_id: policy.policy_id,
            effect: policy.effect.to_string(),
            policy_text: policy.policy_text,
        })
        .collect();

    // Convert diagnostics
    let diagnostics = EvaluationDiagnosticsDto {
        total_policies: result.diagnostics.total_policies,
        matched_policies: result.diagnostics.matched_policies,
        schema_validated: result.diagnostics.schema_validated,
        validation_errors: result.diagnostics.validation_errors,
        warnings: result.diagnostics.warnings,
    };

    PlaygroundEvaluateResponse {
        decision: result.decision.to_string(),
        determining_policies,
        diagnostics,
        errors: result.errors,
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
    fn test_convert_attribute_value_string() {
        let dto = AttributeValueDto::String("test".to_string());
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::String(s) if s == "test"));
    }

    #[test]
    fn test_convert_attribute_value_long() {
        let dto = AttributeValueDto::Long(42);
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::Long(n) if n == 42));
    }

    #[test]
    fn test_convert_attribute_value_bool() {
        let dto = AttributeValueDto::Bool(true);
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::Bool(b) if b));
    }

    #[test]
    fn test_convert_attribute_value_entity_ref_valid() {
        let dto = AttributeValueDto::EntityRef("hodei::iam::default::User::alice".to_string());
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::EntityRef(_)));
    }

    #[test]
    fn test_convert_attribute_value_entity_ref_invalid() {
        let dto = AttributeValueDto::EntityRef("invalid-hrn".to_string());
        let result = convert_attribute_value(dto);
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_attribute_value_set() {
        let dto = AttributeValueDto::Set(vec!["a".to_string(), "b".to_string()]);
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::Set(v) if v.len() == 2));
    }

    #[test]
    fn test_convert_attribute_value_record() {
        let mut record = HashMap::new();
        record.insert("key".to_string(), "value".to_string());

        let dto = AttributeValueDto::Record(record);
        let result = convert_attribute_value(dto).unwrap();
        assert!(matches!(result, AttributeValue::Record(r) if r.len() == 1));
    }

    #[test]
    fn test_convert_to_command_success() {
        let request = PlaygroundEvaluateRequest {
            inline_schema: Some("{}".to_string()),
            schema_version: None,
            inline_policies: vec!["permit(principal, action, resource);".to_string()],
            request: PlaygroundAuthorizationRequestDto {
                principal: "hodei::iam::default::User::alice".to_string(),
                action: "hodei::api::Action::read".to_string(),
                resource: "hodei::storage::default::Document::doc1".to_string(),
                context: HashMap::new(),
            },
        };

        let result = convert_to_command(request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_convert_to_command_invalid_hrn() {
        let request = PlaygroundEvaluateRequest {
            inline_schema: Some("{}".to_string()),
            schema_version: None,
            inline_policies: vec!["permit(principal, action, resource);".to_string()],
            request: PlaygroundAuthorizationRequestDto {
                principal: "invalid-hrn".to_string(),
                action: "hodei::api::Action::read".to_string(),
                resource: "hodei::storage::default::Document::doc1".to_string(),
                context: HashMap::new(),
            },
        };

        let result = convert_to_command(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_to_response() {
        let domain_result = PlaygroundEvaluateResult::new(
            hodei_policies::features::playground_evaluate::dto::Decision::Allow,
            vec![
                hodei_policies::features::playground_evaluate::dto::DeterminingPolicy::new(
                    "policy_0".to_string(),
                    hodei_policies::features::playground_evaluate::dto::PolicyEffect::Permit,
                )
                .with_text("permit(principal, action, resource);".to_string()),
            ],
            hodei_policies::features::playground_evaluate::dto::EvaluationDiagnostics::new(1, 1)
                .with_schema_validation(),
        );

        let response = convert_to_response(domain_result);

        assert_eq!(response.decision, "ALLOW");
        assert_eq!(response.determining_policies.len(), 1);
        assert_eq!(response.diagnostics.total_policies, 1);
        assert_eq!(response.diagnostics.matched_policies, 1);
        assert!(response.diagnostics.schema_validated);
        assert!(response.errors.is_empty());
    }
}
