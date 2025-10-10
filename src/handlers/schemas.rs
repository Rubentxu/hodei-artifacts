//! Schema management handlers
//!
//! This module provides HTTP handlers for schema-related operations:
//! - Building schemas from registered entity and action types
//! - Loading schemas from storage
//! - Registering IAM schemas

use crate::app_state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request to build a schema
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BuildSchemaRequest {
    /// Optional schema version
    pub version: Option<String>,
    /// Whether to validate the schema after building
    #[serde(default = "default_validate")]
    pub validate: bool,
}

fn default_validate() -> bool {
    true
}

/// Response from building a schema
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BuildSchemaResponse {
    /// Number of entity types in the schema
    pub entity_count: usize,
    /// Number of action types in the schema
    pub action_count: usize,
    /// Schema version
    pub version: Option<String>,
    /// Whether the schema was validated
    pub validated: bool,
    /// Schema ID in storage
    pub schema_id: String,
}

/// Request to register IAM schema
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterIamSchemaRequest {
    /// Optional schema version
    pub version: Option<String>,
    /// Whether to validate the schema after building
    #[serde(default = "default_validate")]
    pub validate: bool,
}

/// Response from registering IAM schema
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterIamSchemaResponse {
    /// Number of entity types registered
    pub entity_types_registered: usize,
    /// Number of action types registered
    pub action_types_registered: usize,
    /// Schema version
    pub schema_version: String,
    /// Schema ID in storage
    pub schema_id: String,
    /// Whether the schema was validated
    pub validated: bool,
}

/// Handler to build a schema
///
/// This endpoint builds a Cedar schema from all currently registered
/// entity and action types.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Build schema request parameters
///
/// # Returns
///
/// A JSON response with the build result or an error
#[utoipa::path(
    post,
    path = "/api/v1/schemas/build",
    tag = "schemas",
    request_body = BuildSchemaRequest,
    responses(
        (status = 200, description = "Schema built successfully", body = BuildSchemaResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn build_schema(
    State(state): State<AppState>,
    Json(request): Json<BuildSchemaRequest>,
) -> Result<Json<BuildSchemaResponse>, ApiError> {
    let command = hodei_policies::features::build_schema::dto::BuildSchemaCommand {
        version: request.version,
        validate: request.validate,
    };

    let result = state
        .build_schema
        .execute(command)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("Failed to build schema: {}", e)))?;

    Ok(Json(BuildSchemaResponse {
        entity_count: result.entity_count,
        action_count: result.action_count,
        version: result.version,
        validated: result.validated,
        schema_id: result.schema_id,
    }))
}

/// Handler to load a schema
///
/// This endpoint loads a previously built schema from storage.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
///
/// # Returns
///
/// A JSON response with the loaded schema information or an error
#[utoipa::path(
    get,
    path = "/api/v1/schemas/load",
    tag = "schemas",
    responses(
        (status = 200, description = "Schema loaded successfully", body = serde_json::Value),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn load_schema(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // TODO: Implement schema loading
    // For now, return a stub response
    Ok(Json(serde_json::json!({
        "message": "Load schema endpoint - to be implemented",
        "status": "stub"
    })))
}

/// Handler to register IAM schema
///
/// This endpoint registers all IAM entity types (User, Group) and
/// action types (CreateUser, DeleteUser, etc.) and builds the schema.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
/// * `request` - Register IAM schema request parameters
///
/// # Returns
///
/// A JSON response with the registration result or an error
#[utoipa::path(
    post,
    path = "/api/v1/schemas/register-iam",
    tag = "schemas",
    request_body = RegisterIamSchemaRequest,
    responses(
        (status = 200, description = "IAM schema registered successfully", body = RegisterIamSchemaResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn register_iam_schema(
    State(state): State<AppState>,
    Json(request): Json<RegisterIamSchemaRequest>,
) -> Result<Json<RegisterIamSchemaResponse>, ApiError> {
    let command = hodei_iam::features::register_iam_schema::RegisterIamSchemaCommand::new()
        .with_validation(request.validate);

    let command = if let Some(version) = request.version {
        command.with_version(version)
    } else {
        command
    };

    let result = state
        .register_iam_schema
        .register(command)
        .await
        .map_err(|e| {
            ApiError::InternalServerError(format!("Failed to register IAM schema: {}", e))
        })?;

    Ok(Json(RegisterIamSchemaResponse {
        entity_types_registered: result.entity_types_registered,
        action_types_registered: result.action_types_registered,
        schema_version: result.schema_version,
        schema_id: result.schema_id,
        validated: result.validated,
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
    fn test_build_schema_request_default_validate() {
        let json = r#"{"version": "v1.0.0"}"#;
        let request: BuildSchemaRequest = serde_json::from_str(json).unwrap();
        assert!(request.validate);
        assert_eq!(request.version, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_register_iam_schema_request_serialization() {
        let request = RegisterIamSchemaRequest {
            version: Some("v1.0.0".to_string()),
            validate: true,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("v1.0.0"));
    }
}
