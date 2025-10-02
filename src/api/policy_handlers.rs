use crate::{
    app_state::AppState,
    error::{AppError, Result},
};
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

/// Request body for creating a new policy
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePolicyRequest {
    /// Name of the policy
    #[schema(example = "allow-read-access")]
    pub name: String,
    /// Optional description of the policy
    #[schema(example = "Allows read access to resources")]
    pub description: Option<String>,
    /// Cedar policy content
    #[schema(example = "permit(principal, action == Action::\"read\", resource);")]
    pub policy_content: String,
    /// Whether the policy is enabled
    #[schema(example = true)]
    pub enabled: Option<bool>,
}

/// Response containing policy details
#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyResponse {
    /// Unique identifier of the policy
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub id: String,
    /// Name of the policy
    #[schema(example = "allow-read-access")]
    pub name: String,
    /// Optional description
    #[schema(example = "Allows read access to resources")]
    pub description: Option<String>,
    /// Cedar policy content
    #[schema(example = "permit(principal, action == Action::\"read\", resource);")]
    pub policy_content: String,
    /// Whether the policy is enabled
    pub enabled: bool,
    /// ISO 8601 timestamp of creation
    #[schema(example = "2024-01-01T12:00:00Z")]
    pub created_at: String,
    /// ISO 8601 timestamp of last update
    #[schema(example = "2024-01-01T12:00:00Z")]
    pub updated_at: String,
}

/// Response containing a list of policies
#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyListResponse {
    /// List of policies
    pub policies: Vec<PolicyResponse>,
    /// Total number of policies returned (after pagination)
    pub total: usize,
    /// Offset used for pagination
    pub offset: usize,
    /// Limit used for pagination
    pub limit: Option<usize>,
}

/// Query parameters for listing policies
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct ListPoliciesParams {
    /// Number of items to skip (pagination)
    #[param(example = 0)]
    #[schema(example = 0)]
    pub offset: Option<usize>,
    /// Maximum number of items to return (pagination, max 1000)
    #[param(example = 100)]
    #[schema(example = 100)]
    pub limit: Option<usize>,
    /// Filter policies by ID (partial match)
    #[param(example = "policy")]
    #[schema(example = "policy")]
    pub filter_id: Option<String>,
}

/// Error response
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
    /// Optional error details
    pub details: Option<String>,
}

/// Create a new policy
///
/// Creates a new Cedar policy with the provided content.
#[utoipa::path(
    post,
    path = "/api/v1/policies",
    request_body = CreatePolicyRequest,
    responses(
        (status = 200, description = "Policy created successfully", body = PolicyResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(
        policy_name = %request.name,
        "Creating new policy"
    );

    // Validate request
    if request.name.is_empty() {
        return Err(AppError::BadRequest(
            "Policy name cannot be empty".to_string(),
        ));
    }

    if request.policy_content.is_empty() {
        return Err(AppError::BadRequest(
            "Policy content cannot be empty".to_string(),
        ));
    }

    // Build command and validate via policies DTO
    let cmd = policies::features::create_policy::dto::CreatePolicyCommand::new(
        request.policy_content.clone(),
    );
    if let Err(e) = cmd.validate() {
        return Err(AppError::Validation(e.to_string()));
    }

    // Execute use case from AppState
    state
        .create_policy_uc
        .execute(&cmd)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Create response (ID generated here; persistence layer stores the policy text)
    let policy_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let policy = PolicyResponse {
        id: policy_id.clone(),
        name: request.name.clone(),
        description: request.description,
        policy_content: request.policy_content,
        enabled: request.enabled.unwrap_or(true),
        created_at: now.clone(),
        updated_at: now,
    };

    tracing::info!(
        policy_id = %policy_id,
        policy_name = %request.name,
        "Policy created successfully"
    );

    Ok(Json(policy))
}

/// List all policies
///
/// Retrieves a list of all policies in the system with optional pagination and filtering.
#[utoipa::path(
    get,
    path = "/api/v1/policies",
    params(ListPoliciesParams),
    responses(
        (status = 200, description = "List of policies retrieved successfully", body = PolicyListResponse),
        (status = 400, description = "Invalid query parameters", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListPoliciesParams>,
) -> Result<Json<PolicyListResponse>> {
    tracing::debug!(
        offset = ?params.offset,
        limit = ?params.limit,
        filter_id = ?params.filter_id,
        "Listing policies with parameters"
    );

    // Build query from params
    let mut query = policies::features::list_policies::dto::ListPoliciesQuery::new();
    query.offset = params.offset;
    query.limit = params.limit;
    query.filter_id = params.filter_id;

    // Execute use case from AppState
    let cedar_policies = state
        .list_policies_uc
        .execute(&query)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Convert Vec<Policy> to Vec<PolicyResponse>
    let now = chrono::Utc::now().to_rfc3339();
    let policy_responses: Vec<PolicyResponse> = cedar_policies
        .iter()
        .map(|p| {
            PolicyResponse {
                id: p.id().to_string(),
                name: p.id().to_string(), // Cedar policies use ID as name
                description: None,
                policy_content: p.to_string(),
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            }
        })
        .collect();

    let response = PolicyListResponse {
        total: policy_responses.len(),
        policies: policy_responses,
        offset: query.offset.unwrap_or(0),
        limit: query.limit,
    };

    tracing::info!(
        total_policies = response.total,
        offset = response.offset,
        limit = ?response.limit,
        "Policies listed successfully"
    );

    Ok(Json(response))
}

/// Delete a policy
///
/// Deletes a specific policy by its unique identifier.
#[utoipa::path(
    delete,
    path = "/api/v1/policies/{policy_id}",
    params(
        ("policy_id" = String, Path, description = "Unique identifier of the policy to delete")
    ),
    responses(
        (status = 200, description = "Policy deleted successfully"),
        (status = 400, description = "Invalid policy ID", body = ErrorResponse),
        (status = 404, description = "Policy not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        policy_id = %policy_id,
        "Deleting policy"
    );

    // Build command
    let cmd = policies::features::delete_policy::dto::DeletePolicyCommand::new(policy_id.clone());

    // Execute use case from AppState
    state
        .delete_policy_uc
        .execute(&cmd)
        .await
        .map_err(|e| match e {
            policies::features::delete_policy::use_case::DeletePolicyError::NotFound(_) => {
                AppError::NotFound(format!("Policy with ID '{}' not found", policy_id))
            }
            policies::features::delete_policy::use_case::DeletePolicyError::InvalidCommand(msg) => {
                AppError::BadRequest(msg)
            }
            policies::features::delete_policy::use_case::DeletePolicyError::Storage(msg) => {
                AppError::Internal(msg)
            }
        })?;

    // Record metrics
    state.metrics.record_policy_operation();

    tracing::info!(
        policy_id = %policy_id,
        "Policy deleted successfully"
    );

    Ok(Json(serde_json::json!({
        "message": "Policy deleted successfully",
        "policy_id": policy_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

/// Get a policy by ID
///
/// Retrieves a specific policy by its unique identifier.
#[utoipa::path(
    get,
    path = "/api/v1/policies/{policy_id}",
    params(
        ("policy_id" = String, Path, description = "Unique identifier of the policy")
    ),
    responses(
        (status = 200, description = "Policy found successfully", body = PolicyResponse),
        (status = 404, description = "Policy not found", body = ErrorResponse),
        (status = 400, description = "Invalid policy ID", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(
        policy_id = %policy_id,
        "Getting policy"
    );

    // Validate policy_id
    if policy_id.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Policy ID cannot be empty".to_string(),
        ));
    }

    // Build query
    let query = policies::features::get_policy::dto::GetPolicyQuery::new(policy_id.clone());

    // Execute use case from AppState
    let policy = state
        .get_policy_uc
        .execute(&query)
        .await
        .map_err(|e| match e {
            policies::features::get_policy::use_case::GetPolicyError::NotFound(_) => {
                AppError::NotFound(format!("Policy with ID '{}' not found", policy_id))
            }
            policies::features::get_policy::use_case::GetPolicyError::InvalidQuery(msg) => {
                AppError::BadRequest(msg)
            }
            policies::features::get_policy::use_case::GetPolicyError::Storage(msg) => {
                AppError::Internal(msg)
            }
        })?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Convert Cedar Policy to PolicyResponse
    let now = chrono::Utc::now().to_rfc3339();
    let response = PolicyResponse {
        id: policy.id().to_string(),
        name: policy.id().to_string(), // Cedar policies don't have separate names
        description: None,
        policy_content: policy.to_string(),
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    };

    tracing::info!(
        policy_id = %policy_id,
        "Policy retrieved successfully"
    );

    Ok(Json(response))
}
