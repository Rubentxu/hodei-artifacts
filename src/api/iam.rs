//! IAM API endpoints for managing users and groups

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use hodei_iam::{
    features::{
        create_user::dto::{CreateUserCommand, UserView},
        create_group::dto::{CreateGroupCommand, GroupView},
        add_user_to_group::dto::AddUserToGroupCommand,
    },
};

use crate::{app_state::AppState, error::{AppError, Result}};

// ============================================================================
// DTOs for API
// ============================================================================

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateGroupRequest {
    pub group_name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AddUserToGroupRequest {
    pub user_hrn: String,
    pub group_hrn: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserResponse {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}

impl From<UserView> for UserResponse {
    fn from(v: UserView) -> Self {
        Self { hrn: v.hrn, name: v.name, email: v.email, groups: v.groups, tags: v.tags }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GroupResponse {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

impl From<GroupView> for GroupResponse {
    fn from(v: GroupView) -> Self { Self { hrn: v.hrn, name: v.name, tags: v.tags } }
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new user
#[utoipa::path(
    post,
    path = "/api/v1/iam/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created successfully", body = UserResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "IAM"
)]
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>)> {
    tracing::info!("Creating user: {}", req.name);

    let command = CreateUserCommand {
        name: req.name,
        email: req.email,
        tags: req.tags,
    };

    let view = state.create_user_uc.execute(command).await
        .map_err(|e| AppError::Internal(format!("Failed to create user: {}", e)))?;

    tracing::info!("User created successfully: {}", view.hrn);
    Ok((StatusCode::CREATED, Json(view.into())))
}

/// Create a new group
#[utoipa::path(
    post,
    path = "/api/v1/iam/groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 201, description = "Group created successfully", body = GroupResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "IAM"
)]
pub async fn create_group(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupResponse>)> {
    tracing::info!("Creating group: {}", req.group_name);

    let command = CreateGroupCommand {
        group_name: req.group_name,
        tags: req.tags,
    };

    let view = state.create_group_uc.execute(command).await
        .map_err(|e| AppError::Internal(format!("Failed to create group: {}", e)))?;

    tracing::info!("Group created successfully: {}", view.hrn);
    Ok((StatusCode::CREATED, Json(view.into())))
}

/// Add user to group
#[utoipa::path(
    post,
    path = "/api/v1/iam/groups/members",
    request_body = AddUserToGroupRequest,
    responses(
        (status = 204, description = "User added to group successfully"),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "User or group not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "IAM"
)]
pub async fn add_user_to_group(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddUserToGroupRequest>,
) -> Result<StatusCode> {
    tracing::info!("Adding user {} to group {}", req.user_hrn, req.group_hrn);

    let command = AddUserToGroupCommand {
        user_hrn: req.user_hrn,
        group_hrn: req.group_hrn,
    };

    state.add_user_to_group_uc.execute(command).await
        .map_err(|e| {
            let err_msg = e.to_string();
            if err_msg.contains("not found") {
                AppError::NotFound(err_msg)
            } else {
                AppError::Internal(format!("Failed to add user to group: {}", e))
            }
        })?;

    tracing::info!("User added to group successfully");
    Ok(StatusCode::NO_CONTENT)
}

/// List all users (for debugging/testing)
#[utoipa::path(
    get,
    path = "/api/v1/iam/users",
    responses(
        (status = 200, description = "List of users retrieved successfully", body = [UserResponse]),
        (status = 500, description = "Internal server error")
    ),
    tag = "IAM"
)]
pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<UserResponse>>> {
    tracing::info!("Listing all users");

    let users = state.user_repo.find_all().await
        .map_err(|e| AppError::Internal(format!("Failed to list users: {}", e)))?;

    let views: Vec<UserResponse> = users.into_iter().map(|u| UserResponse {
        hrn: u.hrn.to_string(),
        name: u.name,
        email: u.email,
        groups: u.group_hrns.iter().map(|g| g.to_string()).collect(),
        tags: u.tags,
    }).collect();

    Ok(Json(views))
}

/// List all groups (for debugging/testing)
#[utoipa::path(
    get,
    path = "/api/v1/iam/groups",
    responses(
        (status = 200, description = "List of groups retrieved successfully", body = [GroupResponse]),
        (status = 500, description = "Internal server error")
    ),
    tag = "IAM"
)]
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<GroupResponse>>> {
    tracing::info!("Listing all groups");

    let groups = state.group_repo.find_all().await
        .map_err(|e| AppError::Internal(format!("Failed to list groups: {}", e)))?;

    let views: Vec<GroupResponse> = groups.into_iter().map(|g| GroupResponse {
        hrn: g.hrn.to_string(),
        name: g.name,
        tags: g.tags,
    }).collect();

    Ok(Json(views))
}

// ============================================================================
// Router
// ============================================================================

pub fn iam_routes() -> axum::Router<std::sync::Arc<crate::app_state::AppState>> {
    use axum::routing::post;

    axum::Router::new()
        .route("/users", post(create_user).get(list_users))
        .route("/groups", post(create_group).get(list_groups))
        .route("/groups/members", post(add_user_to_group))
}
