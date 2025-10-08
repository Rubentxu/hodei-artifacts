//! IAM Policy Management Handlers
//!
//! This module provides HTTP handlers for IAM policy management operations:
//! - Create new IAM policies
//! - Get policy details by HRN
//! - List policies with pagination
//! - Update existing policies
//! - Delete policies
//!
//! All handlers follow the Clean Architecture principles and use the
//! appropriate use cases from the hodei-iam crate.

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

/// Request to create a new IAM policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyRequest {
    /// Policy ID (unique identifier within the account)
    pub policy_id: String,
    /// Cedar policy content
    pub policy_content: String,
    /// Optional description of the policy
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from policy creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyResponse {
    /// Policy HRN (Hierarchical Resource Name)
    pub hrn: Hrn,
    /// Policy content
    pub content: String,
    /// Policy description
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to get a policy by HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyRequest {
    /// Policy HRN (Hierarchical Resource Name)
    pub policy_hrn: Hrn,
}

/// Response from getting a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyResponse {
    /// Policy HRN (Hierarchical Resource Name)
    pub hrn: Hrn,
    /// Policy name
    pub name: String,
    /// Policy content
    pub content: String,
    /// Policy description
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Query parameters for listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQueryParams {
    /// Maximum number of policies to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

/// Response from listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    /// List of policies
    pub policies: Vec<PolicySummary>,
    /// Pagination information
    pub page_info: PageInfo,
}

/// Policy summary for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    /// Policy HRN
    pub hrn: Hrn,
    /// Policy name
    pub name: String,
    /// Policy description
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    /// Total number of policies
    pub total_count: usize,
    /// Whether there are more pages
    pub has_next_page: bool,
    /// Whether there are previous pages
    pub has_previous_page: bool,
}

/// Request to update an existing policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyRequest {
    /// Policy HRN (Hierarchical Resource Name)
    pub policy_hrn: Hrn,
    /// New policy content
    pub policy_content: String,
    /// New policy description
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from policy update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyResponse {
    /// Policy HRN (Hierarchical Resource Name)
    pub hrn: Hrn,
    /// Updated policy content
    pub content: String,
    /// Updated policy description
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request to delete a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyRequest {
    /// Policy HRN (Hierarchical Resource Name)
    pub policy_hrn: Hrn,
}

/// Response from policy deletion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyResponse {
    /// Deleted policy HRN
    pub deleted_hrn: Hrn,
    /// Success message
    pub message: String,
}

/// Handler to create a new IAM policy
///
/// This endpoint creates a new IAM policy with the specified ID and content.
/// The policy content is validated for Cedar syntax and semantics.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy creation request
///
/// # Returns
///
/// A JSON response with the created policy details or an error
///
/// # Example Request
///
/// ```json
/// {
///   "policy_id": "allow-read-docs",
///   "policy_content": "permit(principal, action, resource);",
///   "description": "Allow document reading"
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///   "content": "permit(principal, action, resource);",
///   "description": "Allow document reading",
///   "created_at": "2024-01-01T00:00:00Z",
///   "updated_at": "2024-01-01T00:00:00Z"
/// }
/// ```
pub async fn create_policy<S>(
    State(_state): State<AppState<S>>,
    Json(_request): Json<CreatePolicyRequest>,
) -> Result<Json<CreatePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy creation
    // This requires:
    // 1. Creating CreatePolicyCommand from the request
    // 2. Calling the create_policy use case
    // 3. Mapping the result to the response

    // For now, return a stub response
    let hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "stub-policy".to_string(),
    );

    Ok(Json(CreatePolicyResponse {
        hrn,
        content: "permit(principal, action, resource);".to_string(),
        description: Some("Stub policy - implementation pending".to_string()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to get a policy by HRN
///
/// This endpoint retrieves the details of a specific IAM policy by its HRN.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy retrieval request
///
/// # Returns
///
/// A JSON response with the policy details or an error
///
/// # Example Request
///
/// ```json
/// {
///   "policy_hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs"
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///   "name": "allow-read-docs",
///   "content": "permit(principal, action, resource);",
///   "description": "Allow document reading",
///   "created_at": "2024-01-01T00:00:00Z",
///   "updated_at": "2024-01-01T00:00:00Z"
/// }
/// ```
pub async fn get_policy<S>(
    State(_state): State<AppState<S>>,
    Json(_request): Json<GetPolicyRequest>,
) -> Result<Json<GetPolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy retrieval
    // This requires:
    // 1. Creating GetPolicyQuery from the request
    // 2. Calling the get_policy use case
    // 3. Mapping the result to the response

    // For now, return a stub response
    let hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "stub-policy".to_string(),
    );

    Ok(Json(GetPolicyResponse {
        hrn: hrn.clone(),
        name: "stub-policy".to_string(),
        content: "permit(principal, action, resource);".to_string(),
        description: Some("Stub policy - implementation pending".to_string()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to list policies with pagination
///
/// This endpoint lists IAM policies with optional pagination.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `query` - Query parameters for pagination
///
/// # Returns
///
/// A JSON response with the list of policies and pagination info
///
/// # Example Request
///
/// GET /api/v1/iam/policies?limit=10&offset=0
///
/// # Example Response
///
/// ```json
/// {
///   "policies": [
///     {
///       "hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///       "name": "allow-read-docs",
///       "description": "Allow document reading",
///       "created_at": "2024-01-01T00:00:00Z",
///       "updated_at": "2024-01-01T00:00:00Z"
///     }
///   ],
///   "page_info": {
///     "total_count": 1,
///     "has_next_page": false,
///     "has_previous_page": false
///   }
/// }
/// ```
pub async fn list_policies<S>(
    State(_state): State<AppState<S>>,
    Query(_query): Query<ListPoliciesQueryParams>,
) -> Result<Json<ListPoliciesResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy listing
    // This requires:
    // 1. Creating ListPoliciesQuery from the query parameters
    // 2. Calling the list_policies use case
    // 3. Mapping the result to the response

    // For now, return a stub response with empty list
    Ok(Json(ListPoliciesResponse {
        policies: vec![],
        page_info: PageInfo {
            total_count: 0,
            has_next_page: false,
            has_previous_page: false,
        },
    }))
}

/// Handler to update an existing policy
///
/// This endpoint updates the content and/or description of an existing IAM policy.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy update request
///
/// # Returns
///
/// A JSON response with the updated policy details or an error
///
/// # Example Request
///
/// ```json
/// {
///   "policy_hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///   "policy_content": "permit(principal == User::\"alice\", action == Action::\"read\", resource);",
///   "description": "Allow Alice to read documents"
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///   "content": "permit(principal == User::\"alice\", action == Action::\"read\", resource);",
///   "description": "Allow Alice to read documents",
///   "created_at": "2024-01-01T00:00:00Z",
///   "updated_at": "2024-01-02T00:00:00Z"
/// }
/// ```
pub async fn update_policy<S>(
    State(_state): State<AppState<S>>,
    Json(_request): Json<UpdatePolicyRequest>,
) -> Result<Json<UpdatePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy update
    // This requires:
    // 1. Creating UpdatePolicyCommand from the request
    // 2. Calling the update_policy use case
    // 3. Mapping the result to the response

    // For now, return a stub response
    let hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "stub-policy".to_string(),
    );

    Ok(Json(UpdatePolicyResponse {
        hrn,
        content: "permit(principal, action, resource);".to_string(),
        description: Some("Stub policy - implementation pending".to_string()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }))
}

/// Handler to delete a policy
///
/// This endpoint deletes an IAM policy by its HRN.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Policy deletion request
///
/// # Returns
///
/// A JSON response confirming deletion or an error
///
/// # Example Request
///
/// ```json
/// {
///   "policy_hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs"
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "deleted_hrn": "hrn:aws:iam::123456789012:Policy/allow-read-docs",
///   "message": "Policy deleted successfully"
/// }
/// ```
pub async fn delete_policy<S>(
    State(_state): State<AppState<S>>,
    Json(_request): Json<DeletePolicyRequest>,
) -> Result<Json<DeletePolicyResponse>, IamApiError>
where
    S: SchemaStoragePort + Clone + Send + Sync + 'static,
{
    // TODO: Implement policy deletion
    // This requires:
    // 1. Creating DeletePolicyCommand from the request
    // 2. Calling the delete_policy use case
    // 3. Mapping the result to the response

    // For now, return a stub response
    let hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "stub-policy".to_string(),
    );

    Ok(Json(DeletePolicyResponse {
        deleted_hrn: hrn,
        message: "Policy deletion endpoint is a stub".to_string(),
    }))
}

/// IAM API Error type for handler responses
#[derive(Debug)]
pub enum IamApiError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    InternalServerError(String),
}

impl IntoResponse for IamApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            IamApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
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
    fn test_update_policy_request_serialization() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "test-policy".to_string(),
        );

        let request = UpdatePolicyRequest {
            policy_hrn: hrn,
            policy_content: "updated content".to_string(),
            description: Some("Updated description".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-policy"));
        assert!(json.contains("updated content"));
    }

    #[test]
    fn test_iam_api_error_response() {
        let error = IamApiError::BadRequest("Invalid input".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
