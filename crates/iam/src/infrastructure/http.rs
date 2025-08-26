//! HTTP adapters for IAM bounded context
//!
//! Contains HTTP endpoints and request/response handling
//!
//! Following Hexagonal Architecture principles

use std::sync::Arc;
use axum::extract::{State, Path};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use axum::routing::{post, patch};
use serde::{Deserialize, Serialize};
use shared::UserId;
use crate::application::IamApi;
use crate::features::create_policy::CreatePolicyCommand;
use crate::features::create_user::CreateUserCommand;
use crate::features::update_user_attributes::UpdateUserAttributesCommand;

#[derive(Clone)]
pub struct Api {
    iam_api: Arc<IamApi>,
}

impl Api {
    pub fn new(iam_api: Arc<IamApi>) -> Self {
        Self { iam_api }
    }

    pub fn routes(self) -> Router {
        Router::new()
            .route("/users", post(Self::create_user_handler))
            .route("/users/:id/attributes", patch(Self::update_user_attributes_handler))
            .route("/policies", post(Self::create_policy_handler))
            .with_state(self)
    }

    pub async fn create_user_handler(
        State(state): State<Self>,
        Json(command): Json<CreateUserCommand>,
    ) -> Result<Json<String>, (StatusCode, String)> {
        let user_id = state.iam_api.create_user(command).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create user: {}", e),
            )
        })?;
        Ok(Json(user_id.to_string()))
    }

    pub async fn update_user_attributes_handler(
        State(state): State<Self>,
        Path(user_id): Path<UserId>,
        Json(command): Json<UpdateUserAttributesCommand>,
    ) -> impl IntoResponse {
        if command.id != user_id {
            return Err((
                StatusCode::BAD_REQUEST,
                "User ID in path does not match user ID in body".to_string(),
            ));
        }

        state.iam_api.update_user_attributes(command).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update user attributes: {}", e),
            )
        })?;

        Ok(StatusCode::OK)
    }

    pub async fn create_policy_handler(
        State(state): State<Self>,
        Json(command): Json<CreatePolicyCommand>,
    ) -> Result<Json<String>, (StatusCode, String)> {
        let policy_id = state.iam_api.create_policy(command).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create policy: {}", e),
            )
        })?;
        Ok(Json(policy_id.to_string()))
    }
}
