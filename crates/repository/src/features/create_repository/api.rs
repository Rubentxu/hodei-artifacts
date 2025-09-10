// crates/repository/src/features/create_repository/api.rs

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error, instrument};

use shared::hrn::{OrganizationId, UserId};
use crate::domain::RepositoryResult;

use super::dto::{CreateRepositoryCommand, CreateRepositoryResponse};
use super::use_case::CreateRepositoryUseCase;

/// Request DTO para el endpoint de creación de repositorios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepositoryRequest {
    pub name: String,
    pub description: Option<String>,
    pub repo_type: crate::domain::repository::RepositoryType,
    pub format: shared::enums::Ecosystem,
    pub config: super::dto::RepositoryConfigDto,
    pub storage_backend_hrn: Option<String>,
    pub is_public: bool,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// Response DTO para errores del endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepositoryErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<String>,
}

impl CreateRepositoryErrorResponse {
    pub fn new(error: String, message: String, details: Option<String>) -> Self {
        Self { error, message, details }
    }

    pub fn validation_error(message: String) -> Self {
        Self::new("ValidationError".to_string(), message, None)
    }

    pub fn not_found(message: String) -> Self {
        Self::new("NotFound".to_string(), message, None)
    }

    pub fn conflict(message: String) -> Self {
        Self::new("Conflict".to_string(), message, None)
    }

    pub fn internal_error(message: String, details: Option<String>) -> Self {
        Self::new("InternalError".to_string(), message, details)
    }
}

/// Punto de entrada de la API para crear repositorios
pub struct CreateRepositoryEndpoint {
    use_case: Arc<CreateRepositoryUseCase>,
}

impl CreateRepositoryEndpoint {
    pub fn new(use_case: Arc<CreateRepositoryUseCase>) -> Self {
        Self { use_case }
    }

    #[instrument(skip(self, request, organization_id, user_id))]
    pub async fn create_repository(
        &self,
        request: CreateRepositoryRequest,
        organization_id: OrganizationId,
        user_id: UserId,
    ) -> Result<CreateRepositoryResponse, CreateRepositoryErrorResponse> {
        info!("Creating repository '{}' for organization '{}'", request.name, organization_id.as_str());

        // Convertir el request DTO al command DTO
        let command = CreateRepositoryCommand {
            name: request.name,
            description: request.description,
            repo_type: request.repo_type,
            format: request.format,
            config: request.config,
            storage_backend_hrn: request.storage_backend_hrn,
            is_public: request.is_public,
            metadata: request.metadata,
        };

        // Ejecutar el caso de uso
        match self.use_case.execute(command, organization_id, user_id).await {
            Ok(response) => {
                info!("Repository '{}' created successfully", response.name);
                Ok(response)
            },
            Err(error) => {
                error!("Failed to create repository: {}", error);
                Err(Self::map_error_to_response(error))
            }
        }
    }

    fn map_error_to_response(error: crate::domain::RepositoryError) -> CreateRepositoryErrorResponse {
        use crate::domain::RepositoryError;

        match error {
            RepositoryError::RepositoryAlreadyExists(name) => {
                CreateRepositoryErrorResponse::conflict(format!("Repository '{}' already exists", name))
            },
            RepositoryError::OrganizationNotFound(org_id) => {
                CreateRepositoryErrorResponse::not_found(format!("Organization '{}' not found", org_id))
            },
            RepositoryError::InvalidRepositoryName(message) => {
                CreateRepositoryErrorResponse::validation_error(format!("Invalid repository name: {}", message))
            },
            RepositoryError::InvalidConfiguration(message) => {
                CreateRepositoryErrorResponse::validation_error(format!("Invalid configuration: {}", message))
            },
            RepositoryError::RepositoryTypeMismatch { expected, actual } => {
                CreateRepositoryErrorResponse::validation_error(
                    format!("Repository type mismatch: expected {}, got {}", expected, actual)
                )
            },
            RepositoryError::StorageBackendNotFound(backend) => {
                CreateRepositoryErrorResponse::not_found(format!("Storage backend '{}' not found", backend))
            },
            RepositoryError::ReferencedRepositoryNotFound(repo_id) => {
                CreateRepositoryErrorResponse::not_found(format!("Referenced repository '{}' not found", repo_id))
            },
            RepositoryError::Unauthorized(message) => {
                CreateRepositoryErrorResponse::new("Unauthorized".to_string(), message, None)
            },
            RepositoryError::DatabaseError(message) => {
                CreateRepositoryErrorResponse::internal_error(
                    "Database operation failed".to_string(),
                    Some(message)
                )
            },
            RepositoryError::ValidationError(message) => {
                CreateRepositoryErrorResponse::validation_error(message)
            },
            RepositoryError::ConfigurationError(message) => {
                CreateRepositoryErrorResponse::validation_error(format!("Configuration error: {}", message))
            },
            RepositoryError::RepositoryNotEmpty(repo_id) => {
                CreateRepositoryErrorResponse::conflict(
                    format!("Repository '{}' is not empty and cannot be deleted", repo_id)
                )
            },
        }
    }
}

/// Handler de Axum para el endpoint de creación de repositorios
pub async fn create_repository_handler(
    Extension(endpoint): Extension<Arc<CreateRepositoryEndpoint>>,
    Extension(organization_id): Extension<OrganizationId>,
    Extension(user_id): Extension<UserId>,
    Json(request): Json<CreateRepositoryRequest>,
) -> impl IntoResponse {
    match endpoint.create_repository(request, organization_id, user_id).await {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(error_response) => {
            let status_code = match error_response.error.as_str() {
                "ValidationError" => StatusCode::BAD_REQUEST,
                "NotFound" => StatusCode::NOT_FOUND,
                "Conflict" => StatusCode::CONFLICT,
                "Unauthorized" => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status_code, Json(error_response)).into_response()
        }
    }
}