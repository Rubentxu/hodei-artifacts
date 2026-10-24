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
    let command = hodei_policies::build_schema::dto::BuildSchemaCommand {
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

/// Status of the schema load response
///
/// The endpoint answers with an explicit status instead of a placeholder, so
/// a consumer can tell "there is no schema" from "there is a schema and this
/// is it" without inspecting the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadSchemaStatus {
    /// A schema is active; its version and content are returned
    Active,
    /// No schema is stored, so there is nothing to load
    NoActiveSchema,
}

/// Response from loading the active schema
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoadSchemaResponse {
    /// Whether an active schema was found
    pub status: LoadSchemaStatus,
    /// The version the active schema is stored under
    pub version: Option<String>,
    /// The identifier of the active schema in storage
    pub schema_id: Option<String>,
    /// The persisted content of the active schema
    ///
    /// It is the `cedar-schema-fragments-v1` envelope exactly as it is
    /// stored: Cedar cannot serialize a composed schema back, so the envelope
    /// is the faithful rendering of the stored schema.
    pub content: Option<String>,
    /// Number of entity types declared by the active schema
    pub entity_type_count: usize,
    /// Number of action types declared by the active schema
    pub action_type_count: usize,
}

/// Handler to load the active schema
///
/// This endpoint reads the schema that is currently stored and reports it:
/// the version it is stored under and its content.
///
/// # Arguments
///
/// * `state` - Application state containing use cases
///
/// # Returns
///
/// A JSON response with the active schema, or an explicit
/// `no_active_schema` status when nothing is stored
///
/// # Errors
///
/// Returns an error if the storage is unavailable or the stored content
/// cannot be read as a schema.
#[utoipa::path(
    get,
    path = "/api/v1/schemas/load",
    tag = "schemas",
    responses(
        (status = 200, description = "Schema loaded successfully", body = LoadSchemaResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn load_schema(
    State(state): State<AppState>,
) -> Result<Json<LoadSchemaResponse>, ApiError> {
    use hodei_policies::load_schema::LoadSchemaError;
    use hodei_policies::load_schema::dto::LoadSchemaCommand;

    let result = match state.load_schema.execute(LoadSchemaCommand::latest()).await {
        Ok(result) => result,
        // Reading succeeded and the answer is "there is no schema": that is
        // not a failure, and the consumer has to be able to distinguish it
        // from an error.
        Err(LoadSchemaError::SchemaNotFound) => {
            return Ok(Json(LoadSchemaResponse {
                status: LoadSchemaStatus::NoActiveSchema,
                version: None,
                schema_id: None,
                content: None,
                entity_type_count: 0,
                action_type_count: 0,
            }));
        }
        Err(e) => {
            return Err(ApiError::InternalServerError(format!(
                "Failed to load schema: {}",
                e
            )));
        }
    };

    let entity_type_count = result.schema.entity_types().count();
    let action_type_count = result.schema.actions().count();

    Ok(Json(LoadSchemaResponse {
        status: LoadSchemaStatus::Active,
        version: result.version,
        schema_id: Some(result.schema_id),
        content: Some(result.content),
        entity_type_count,
        action_type_count,
    }))
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
