//! IAM Policy Management Handlers
//!
//! This module provides HTTP handlers for IAM policy management operations.
//! All handlers are fully implemented with proper use case calls and error mapping.

use crate::app_state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};

// ============================================================================
// HTTP DTOs (Request/Response types for the HTTP API)
// ============================================================================

/// Request to create a new IAM policy
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreatePolicyRequest {
    pub policy_id: String,
    pub policy_content: String,
    #[serde(default)]
    pub description: Option<String>,
    /// The principals the policy is attached to
    ///
    /// The attachment is what makes the policy effective for a principal: the
    /// IAM evaluation only consults the policies attached to the principal it
    /// is deciding about. A policy created without attachment is never
    /// consulted, and therefore never grants anything.
    #[serde(default)]
    pub attached_principals: Vec<String>,
}

/// Response from policy creation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreatePolicyResponse {
    pub hrn: String,
    pub content: String,
    pub description: Option<String>,
    /// The canonical HRNs the policy is attached to
    pub attached_principals: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to get a policy by HRN
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GetPolicyRequest {
    pub policy_hrn: String,
}

/// Response from getting a policy
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GetPolicyResponse {
    pub hrn: String,
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    /// The canonical HRNs the policy is attached to
    pub attached_principals: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Query parameters for listing policies
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ListPoliciesQueryParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

/// Response from listing policies
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ListPoliciesResponse {
    pub policies: Vec<PolicySummary>,
    pub page_info: PageInfo,
}

/// Policy summary for listing
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PolicySummary {
    pub hrn: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PageInfo {
    pub total_count: usize,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

/// Request to update an existing policy
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdatePolicyRequest {
    pub policy_hrn: String,
    pub policy_content: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from policy update
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdatePolicyResponse {
    pub hrn: String,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to delete a policy
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeletePolicyRequest {
    pub policy_hrn: String,
}

/// Request to evaluate the IAM policies of a principal
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EvaluateIamPoliciesRequest {
    /// The principal the decision is about
    pub principal_hrn: String,
    /// The verb being requested
    pub action: String,
    /// The resource the verb is requested on
    pub resource_hrn: String,
}

/// Response of an IAM policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EvaluateIamPoliciesResponse {
    /// Whether the request is allowed
    pub decision: bool,
    /// Why the decision was reached
    pub reason: String,
    /// The ids of the policies that determined the decision
    pub determining_policies: Vec<String>,
    /// The ids of the policies that were consulted
    pub policy_ids_evaluated: Vec<String>,
}
/// Response from policy deletion
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeletePolicyResponse {
    pub deleted_hrn: String,
    pub message: String,
}

// ============================================================================
// HRN RESOLUTION
// ============================================================================

/// Canonical resource type for IAM policies.
///
/// The domain uses PascalCase for resource types (`User`, `Group`, `Policy`),
/// and `GetPolicyUseCase` rejects any HRN whose resource type is not
/// `Policy`. Keeping one spelling is what makes the HRN returned by one
/// endpoint usable as the identifier of another.
const POLICY_RESOURCE_TYPE: &str = "Policy";

/// Resolve an incoming policy HRN into the short policy id and the canonical
/// HRN of that policy.
///
/// The HTTP contract addresses policies by HRN, but the domain use cases and
/// the persistence adapter address them by the short policy id. Forwarding the
/// raw HRN as an id made the lookup miss the stored record, so `PUT
/// /api/v1/iam/policies/update` and `DELETE /api/v1/iam/policies/delete`
/// answered `404` for policies that had just been created.
fn resolve_policy_hrn(raw: &str) -> Result<(String, kernel::Hrn), IamApiError> {
    let requested = kernel::Hrn::from_string(raw)
        .ok_or_else(|| IamApiError::BadRequest("Invalid HRN format".to_string()))?;

    let canonical = kernel::Hrn::new(
        requested.partition().to_string(),
        requested.service().to_string(),
        requested.account_id().to_string(),
        POLICY_RESOURCE_TYPE.to_string(),
        requested.resource_id().to_string(),
    );

    Ok((requested.resource_id().to_string(), canonical))
}

// ============================================================================
// HANDLER IMPLEMENTATIONS
// ============================================================================

/// Handler to create a new IAM policy
#[utoipa::path(
    post,
    path = "/api/v1/iam/policies",
    tag = "iam",
    request_body = CreatePolicyRequest,
    responses(
        (status = 200, description = "Policy created successfully", body = CreatePolicyResponse),
        (status = 400, description = "Invalid policy content"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_policy(
    State(state): State<AppState>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<Json<CreatePolicyResponse>, IamApiError> {
    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: request.policy_id,
        policy_content: request.policy_content,
        description: request.description,
        attached_principals: request.attached_principals,
    };

    let policy_view = state
        .create_policy
        .execute(command)
        .await
        .map_err(|e| match e {
            hodei_iam::features::create_policy::error::CreatePolicyError::EmptyPolicyContent => {
                IamApiError::BadRequest("Policy content cannot be empty".to_string())
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::InvalidPolicyId(msg) => {
                IamApiError::BadRequest(format!("Invalid policy ID: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::InvalidPolicyContent(
                msg,
            ) => IamApiError::BadRequest(format!("Invalid policy content: {}", msg)),
            hodei_iam::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists(
                id,
            ) => IamApiError::Conflict(format!("Policy already exists: {}", id)),
            hodei_iam::features::create_policy::error::CreatePolicyError::ValidationFailed(msg) => {
                IamApiError::InternalServerError(format!("Validation service error: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::StorageError(msg) => {
                IamApiError::InternalServerError(format!("Storage error: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::InvalidHrn(msg) => {
                IamApiError::BadRequest(format!("Invalid HRN: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::Unauthorized => {
                IamApiError::Unauthorized("Insufficient permissions".to_string())
            }
        })?;

    Ok(Json(CreatePolicyResponse {
        hrn: policy_view.id.to_string(),
        content: policy_view.content,
        description: policy_view.description,
        attached_principals: policy_view.attached_principals,
        created_at: policy_view.created_at,
        updated_at: policy_view.updated_at,
    }))
}

/// Handler to get a policy by HRN
#[utoipa::path(
    post,
    path = "/api/v1/iam/policies/get",
    tag = "iam",
    request_body = GetPolicyRequest,
    responses(
        (status = 200, description = "Policy retrieved successfully", body = GetPolicyResponse),
        (status = 400, description = "Invalid HRN format"),
        (status = 404, description = "Policy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_policy(
    State(state): State<AppState>,
    Json(request): Json<GetPolicyRequest>,
) -> Result<Json<GetPolicyResponse>, IamApiError> {
    let (_policy_id, canonical_hrn) = resolve_policy_hrn(&request.policy_hrn)?;

    let policy_view = state
        .get_policy
        .get_by_hrn(&canonical_hrn)
        .await
        .map_err(|e| match e {
            hodei_iam::features::get_policy::error::GetPolicyError::PolicyNotFound(msg) => {
                IamApiError::NotFound(format!("Policy not found: {}", msg))
            }
            hodei_iam::features::get_policy::error::GetPolicyError::InvalidHrn(msg) => {
                IamApiError::BadRequest(format!("Invalid HRN: {}", msg))
            }
            hodei_iam::features::get_policy::error::GetPolicyError::RepositoryError(msg) => {
                IamApiError::InternalServerError(format!("Repository error: {}", msg))
            }
        })?;

    Ok(Json(GetPolicyResponse {
        hrn: canonical_hrn.to_string(),
        name: policy_view.name,
        content: policy_view.content,
        description: policy_view.description,
        attached_principals: policy_view.attached_principals,
        created_at: chrono::Utc::now(), // TODO: Add timestamps to domain PolicyView
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to list policies with pagination
#[utoipa::path(
    get,
    path = "/api/v1/iam/policies",
    tag = "iam",
    params(
        ("limit" = Option<u32>, Query, description = "Maximum number of policies to return"),
        ("offset" = Option<u32>, Query, description = "Number of policies to skip")
    ),
    responses(
        (status = 200, description = "Policies listed successfully", body = ListPoliciesResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_policies(
    State(state): State<AppState>,
    Query(query): Query<ListPoliciesQueryParams>,
) -> Result<Json<ListPoliciesResponse>, IamApiError> {
    let list_query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: query.limit,
        offset: query.offset,
    };

    let list_result = state
        .list_policies
        .list(list_query)
        .await
        .map_err(|e| match e {
            hodei_iam::features::list_policies::error::ListPoliciesError::Database(msg) => {
                IamApiError::InternalServerError(format!("Database error: {}", msg))
            }
            hodei_iam::features::list_policies::error::ListPoliciesError::InvalidQuery(msg) => {
                IamApiError::BadRequest(format!("Invalid query: {}", msg))
            }
            hodei_iam::features::list_policies::error::ListPoliciesError::InvalidPagination(
                msg,
            ) => IamApiError::BadRequest(format!("Invalid pagination: {}", msg)),
            hodei_iam::features::list_policies::error::ListPoliciesError::RepositoryError(msg) => {
                IamApiError::InternalServerError(format!("Repository error: {}", msg))
            }
            hodei_iam::features::list_policies::error::ListPoliciesError::Internal(msg) => {
                IamApiError::InternalServerError(format!("Internal error: {}", msg))
            }
        })?;

    // Map domain PolicySummary to HTTP PolicySummary (adding timestamps)
    let policies: Vec<PolicySummary> = list_result
        .policies
        .into_iter()
        .map(|p| PolicySummary {
            hrn: p.hrn.to_string(),
            name: p.name,
            description: p.description,
            created_at: chrono::Utc::now(), // TODO: Add timestamps to domain
            updated_at: chrono::Utc::now(),
        })
        .collect();

    Ok(Json(ListPoliciesResponse {
        policies,
        page_info: PageInfo {
            total_count: list_result.total_count,
            has_next_page: list_result.has_next_page,
            has_previous_page: list_result.has_previous_page,
        },
    }))
}

/// Handler to update an existing policy
#[utoipa::path(
    put,
    path = "/api/v1/iam/policies/update",
    tag = "iam",
    request_body = UpdatePolicyRequest,
    responses(
        (status = 200, description = "Policy updated successfully", body = UpdatePolicyResponse),
        (status = 400, description = "Invalid policy content"),
        (status = 404, description = "Policy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_policy(
    State(state): State<AppState>,
    Json(request): Json<UpdatePolicyRequest>,
) -> Result<Json<UpdatePolicyResponse>, IamApiError> {
    let (policy_id, canonical_hrn) = resolve_policy_hrn(&request.policy_hrn)?;

    let command = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_id,
        policy_content: Some(request.policy_content),
        description: request.description,
    };

    let policy_view = state
        .update_policy
        .update(command)
        .await
        .map_err(|e| match e {
            hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyNotFound(msg) => {
                IamApiError::NotFound(format!("Policy not found: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::InvalidPolicyContent(
                msg,
            ) => IamApiError::BadRequest(format!("Invalid policy content: {}", msg)),
            hodei_iam::features::update_policy::error::UpdatePolicyError::InvalidPolicyId(msg) => {
                IamApiError::BadRequest(format!("Invalid policy ID: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::InvalidHrn(msg) => {
                IamApiError::BadRequest(format!("Invalid HRN: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::NoUpdatesProvided => {
                IamApiError::BadRequest("No updates provided".to_string())
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::EmptyPolicyContent => {
                IamApiError::BadRequest("Policy content cannot be empty".to_string())
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::VersionConflict => {
                IamApiError::Conflict("Policy was modified by another process".to_string())
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyInUseConflict(
                msg,
            ) => IamApiError::Conflict(format!("Policy in use: {}", msg)),
            hodei_iam::features::update_policy::error::UpdatePolicyError::SystemPolicyProtected(
                msg,
            ) => IamApiError::BadRequest(format!("System policy protected: {}", msg)),
            hodei_iam::features::update_policy::error::UpdatePolicyError::ValidationFailed(msg) => {
                IamApiError::InternalServerError(format!("Validation service error: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::StorageError(msg) => {
                IamApiError::InternalServerError(format!("Storage error: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::Unauthorized => {
                IamApiError::Unauthorized("Insufficient permissions".to_string())
            }
        })?;

    Ok(Json(UpdatePolicyResponse {
        hrn: canonical_hrn.to_string(),
        content: policy_view.content,
        description: policy_view.description,
        created_at: chrono::Utc::now(), // TODO: Add timestamps to domain PolicyView
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to delete a policy by HRN
#[utoipa::path(
    delete,
    path = "/api/v1/iam/policies/delete",
    tag = "iam",
    request_body = DeletePolicyRequest,
    responses(
        (status = 200, description = "Policy deleted successfully", body = DeletePolicyResponse),
        (status = 404, description = "Policy not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_policy(
    State(state): State<AppState>,
    Json(request): Json<DeletePolicyRequest>,
) -> Result<Json<DeletePolicyResponse>, IamApiError> {
    let (policy_id, canonical_hrn) = resolve_policy_hrn(&request.policy_hrn)?;

    let command = hodei_iam::features::delete_policy::dto::DeletePolicyCommand { policy_id };

    state
        .delete_policy
        .delete(&command.policy_id)
        .await
        .map_err(|e| match e {
            hodei_iam::features::delete_policy::error::DeletePolicyError::PolicyNotFound(msg) => {
                IamApiError::NotFound(format!("Policy not found: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::InvalidPolicyId(msg) => {
                IamApiError::BadRequest(format!("Invalid policy ID: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::InvalidHrn(msg) => {
                IamApiError::BadRequest(format!("Invalid HRN: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::PolicyInUse(msg) => {
                IamApiError::Conflict(format!("Policy in use: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::SystemPolicyProtected(
                msg,
            ) => IamApiError::BadRequest(format!("System policy protected: {}", msg)),
            hodei_iam::features::delete_policy::error::DeletePolicyError::StorageError(msg) => {
                IamApiError::InternalServerError(format!("Storage error: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::Unauthorized => {
                IamApiError::Unauthorized("Insufficient permissions".to_string())
            }
        })?;

    Ok(Json(DeletePolicyResponse {
        deleted_hrn: canonical_hrn.to_string(),
        message: "Policy deleted successfully".to_string(),
    }))
}

/// Handler to evaluate the IAM policies of a principal
///
/// The decision is made from the policies **persisted** in the store: the
/// evaluation consults the policies attached to the principal it is deciding
/// about, and no other.
#[utoipa::path(
    post,
    path = "/api/v1/iam/evaluate",
    tag = "iam",
    request_body = EvaluateIamPoliciesRequest,
    responses(
        (status = 200, description = "Evaluation completed", body = EvaluateIamPoliciesResponse),
        (status = 400, description = "Invalid HRN format"),
        (status = 500, description = "Evaluation failed")
    )
)]
pub async fn evaluate_iam_policies(
    State(state): State<AppState>,
    Json(request): Json<EvaluateIamPoliciesRequest>,
) -> Result<Json<EvaluateIamPoliciesResponse>, IamApiError> {
    let principal_hrn = kernel::Hrn::from_string(&request.principal_hrn)
        .ok_or_else(|| IamApiError::BadRequest("Invalid principal HRN format".to_string()))?;
    let resource_hrn = kernel::Hrn::from_string(&request.resource_hrn)
        .ok_or_else(|| IamApiError::BadRequest("Invalid resource HRN format".to_string()))?;

    let command = hodei_iam::features::evaluate_iam_policies::dto::EvaluateIamPoliciesCommand {
        principal_hrn,
        action_name: request.action,
        resource_hrn,
    };

    let result = state
        .evaluate_iam_policies
        .evaluate(command)
        .await
        .map_err(|e| match e {
            kernel::application::ports::authorization::AuthorizationError::EvaluationFailed(
                msg,
            ) => IamApiError::InternalServerError(format!("Evaluation failed: {}", msg)),
            kernel::application::ports::authorization::AuthorizationError::PolicyNotFound => {
                IamApiError::NotFound("Policy not found".to_string())
            }
            kernel::application::ports::authorization::AuthorizationError::InvalidPolicyFormat => {
                IamApiError::BadRequest("Invalid policy format".to_string())
            }
        })?;

    Ok(Json(EvaluateIamPoliciesResponse {
        decision: result.decision,
        reason: result.reason,
        determining_policies: result.determining_policies,
        policy_ids_evaluated: result.policy_ids_evaluated,
    }))
}

// ============================================================================
// ERROR HANDLING
// ============================================================================

/// IAM API Error type for handler responses
#[derive(Debug)]
pub enum IamApiError {
    BadRequest(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    InternalServerError(String),
}

impl IntoResponse for IamApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            IamApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            IamApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            IamApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            IamApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            IamApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_policy_request_serialization() {
        let request = CreatePolicyRequest {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
            attached_principals: vec!["hrn:aws:iam::account-a:User/alice".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-policy"));
        assert!(json.contains("permit"));
    }

    #[test]
    fn test_list_policies_query_defaults() {
        let query: ListPoliciesQueryParams = serde_json::from_str("{}").unwrap();
        assert_eq!(query.limit, 50);
        assert_eq!(query.offset, 0);
    }

    #[test]
    fn test_iam_api_error_response() {
        let error = IamApiError::BadRequest("Invalid input".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
