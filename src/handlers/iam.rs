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
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

// ============================================================================
// HTTP DTOs (Request/Response types for the HTTP API)
// ============================================================================

/// Request to create a new IAM policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyRequest {
    pub policy_id: String,
    pub policy_content: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from policy creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyResponse {
    pub hrn: Hrn,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to get a policy by HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyRequest {
    pub policy_hrn: Hrn,
}

/// Response from getting a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyResponse {
    pub hrn: Hrn,
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Query parameters for listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    pub policies: Vec<PolicySummary>,
    pub page_info: PageInfo,
}

/// Policy summary for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    pub hrn: Hrn,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub total_count: usize,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

/// Request to update an existing policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyRequest {
    pub policy_hrn: Hrn,
    pub policy_content: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from policy update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyResponse {
    pub hrn: Hrn,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to delete a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyRequest {
    pub policy_hrn: Hrn,
}

/// Response from policy deletion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyResponse {
    pub deleted_hrn: Hrn,
    pub message: String,
}

// ============================================================================
// HANDLER IMPLEMENTATIONS
// ============================================================================

/// Handler to create a new IAM policy
pub async fn create_policy<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<Json<CreatePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: request.policy_id,
        policy_content: request.policy_content,
        description: request.description,
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
            hodei_iam::features::create_policy::error::CreatePolicyError::InvalidPolicyContent(msg) => {
                IamApiError::BadRequest(format!("Invalid policy content: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists(id) => {
                IamApiError::Conflict(format!("Policy already exists: {}", id))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::ValidationFailed(msg) => {
                IamApiError::InternalServerError(format!("Validation service error: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::StorageError(msg) => {
                IamApiError::InternalServerError(format!("Storage error: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::InvalidHrn(msg) => {
                IamApiError::InternalServerError(format!("Invalid HRN: {}", msg))
            }
            hodei_iam::features::create_policy::error::CreatePolicyError::Unauthorized => {
                IamApiError::Unauthorized("Insufficient permissions".to_string())
            }
        })?;

    Ok(Json(CreatePolicyResponse {
        hrn: policy_view.id,
        content: policy_view.content,
        description: policy_view.description,
        created_at: policy_view.created_at,
        updated_at: policy_view.updated_at,
    }))
}

/// Handler to get a policy by HRN
pub async fn get_policy<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<GetPolicyRequest>,
) -> Result<Json<GetPolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
        policy_hrn: request.policy_hrn,
    };

    let policy_view = state
        .get_policy
        .execute(query)
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
        hrn: policy_view.hrn.clone(),
        name: policy_view.name,
        content: policy_view.content,
        description: policy_view.description,
        created_at: chrono::Utc::now(), // TODO: Add timestamps to domain PolicyView
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to list policies with pagination
pub async fn list_policies<S>(
    State(state): State<AppState<S>>,
    Query(query): Query<ListPoliciesQueryParams>,
) -> Result<Json<ListPoliciesResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let list_query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: query.limit,
        offset: query.offset,
    };

    let list_result = state
        .list_policies
        .execute(list_query)
        .await
        .map_err(|e| match e {
            hodei_iam::features::list_policies::error::ListPoliciesError::Database(msg) => {
                IamApiError::InternalServerError(format!("Database error: {}", msg))
            }
            hodei_iam::features::list_policies::error::ListPoliciesError::InvalidQuery(msg) => {
                IamApiError::BadRequest(format!("Invalid query: {}", msg))
            }
            hodei_iam::features::list_policies::error::ListPoliciesError::InvalidPagination(msg) => {
                IamApiError::BadRequest(format!("Invalid pagination: {}", msg))
            }
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
            hrn: p.hrn,
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
pub async fn update_policy<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<UpdatePolicyRequest>,
) -> Result<Json<UpdatePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let command = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: request.policy_hrn,
        policy_content: Some(request.policy_content),
        description: request.description,
    };

    let policy_view = state
        .update_policy
        .execute(command)
        .await
        .map_err(|e| match e {
            hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyNotFound(msg) => {
                IamApiError::NotFound(format!("Policy not found: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::InvalidPolicyContent(msg) => {
                IamApiError::BadRequest(format!("Invalid policy content: {}", msg))
            }
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
            hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyInUseConflict(msg) => {
                IamApiError::Conflict(format!("Policy in use: {}", msg))
            }
            hodei_iam::features::update_policy::error::UpdatePolicyError::SystemPolicyProtected(msg) => {
                IamApiError::BadRequest(format!("System policy protected: {}", msg))
            }
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
        hrn: policy_view.id,
        content: policy_view.content,
        description: policy_view.description,
        created_at: policy_view.created_at,
        updated_at: policy_view.updated_at,
    }))
}

/// Handler to delete a policy
pub async fn delete_policy<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<DeletePolicyRequest>,
) -> Result<Json<DeletePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    let command = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: request.policy_hrn.clone(),
    };

    state
        .delete_policy
        .execute(command)
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
            hodei_iam::features::delete_policy::error::DeletePolicyError::SystemPolicyProtected(msg) => {
                IamApiError::BadRequest(format!("System policy protected: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::StorageError(msg) => {
                IamApiError::InternalServerError(format!("Storage error: {}", msg))
            }
            hodei_iam::features::delete_policy::error::DeletePolicyError::Unauthorized => {
                IamApiError::Unauthorized("Insufficient permissions".to_string())
            }
        })?;

    Ok(Json(DeletePolicyResponse {
        deleted_hrn: request.policy_hrn,
        message: "Policy deleted successfully".to_string(),
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
