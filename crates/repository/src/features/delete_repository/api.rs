// crates/repository/src/features/delete_repository/api.rs

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error, instrument};

use shared::hrn::{RepositoryId, UserId};
use crate::domain::RepositoryResult;

use super::dto::{DeleteRepositoryCommand, DeleteRepositoryResponse};
use super::use_case::DeleteRepositoryUseCase;

/// Query parameters para el endpoint de eliminación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRepositoryQuery {
    /// Indica si se debe forzar la eliminación incluso si el repositorio no está vacío
    pub force: Option<bool>,
}

/// Response DTO para errores del endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRepositoryErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<String>,
}

impl DeleteRepositoryErrorResponse {
    pub fn new(error: String, message: String, details: Option<String>) -> Self {
        Self { error, message, details }
    }

    pub fn not_found(message: String) -> Self {
        Self::new("NotFound".to_string(), message, None)
    }

    pub fn unauthorized(message: String) -> Self {
        Self::new("Unauthorized".to_string(), message, None)
    }

    pub fn invalid_request(message: String) -> Self {
        Self::new("InvalidRequest".to_string(), message, None)
    }

    pub fn internal_error(message: String, details: Option<String>) -> Self {
        Self::new("InternalError".to_string(), message, details)
    }

    pub fn conflict(message: String) -> Self {
        Self::new("Conflict".to_string(), message, None)
    }

    pub fn not_empty(message: String) -> Self {
        Self::new("RepositoryNotEmpty".to_string(), message, None)
    }
}

/// Punto de entrada de la API para eliminar repositorios
pub struct DeleteRepositoryEndpoint {
    use_case: Arc<DeleteRepositoryUseCase>,
}

impl DeleteRepositoryEndpoint {
    pub fn new(use_case: Arc<DeleteRepositoryUseCase>) -> Self {
        Self { use_case }
    }

    #[instrument(skip(self, repository_hrn, query, user_id))]
    pub async fn delete_repository(
        &self,
        repository_hrn: String,
        query: DeleteRepositoryQuery,
        user_id: UserId,
    ) -> Result<DeleteRepositoryResponse, DeleteRepositoryErrorResponse> {
        info!("Deleting repository with HRN: {}", repository_hrn);

        // Construir el comando
        let command = DeleteRepositoryCommand {
            repository_hrn,
            force: query.force.unwrap_or(false),
        };

        // Ejecutar el caso de uso
        match self.use_case.execute(command, user_id).await {
            Ok(response) => {
                info!("Successfully deleted repository: {}", response.hrn);
                Ok(response)
            },
            Err(error) => {
                error!("Failed to delete repository: {}", error);
                Err(Self::map_error_to_response(error))
            }
        }
    }

    fn map_error_to_response(error: crate::domain::RepositoryError) -> DeleteRepositoryErrorResponse {
        use crate::domain::RepositoryError;

        match error {
            RepositoryError::RepositoryNotFound(repo_id) => {
                DeleteRepositoryErrorResponse::not_found(format!("Repository '{}' not found", repo_id))
            },
            RepositoryError::Unauthorized(message) => {
                DeleteRepositoryErrorResponse::unauthorized(message)
            },
            RepositoryError::InvalidRepositoryName(message) => {
                DeleteRepositoryErrorResponse::invalid_request(format!("Invalid repository name: {}", message))
            },
            RepositoryError::DatabaseError(message) => {
                DeleteRepositoryErrorResponse::internal_error(
                    "Database operation failed".to_string(),
                    Some(message)
                )
            },
            RepositoryError::ValidationError(message) => {
                DeleteRepositoryErrorResponse::invalid_request(message)
            },
            RepositoryError::ConfigurationError(message) => {
                DeleteRepositoryErrorResponse::invalid_request(format!("Configuration error: {}", message))
            },
            RepositoryError::OrganizationNotFound(org_id) => {
                DeleteRepositoryErrorResponse::not_found(format!("Organization '{}' not found", org_id))
            },
            RepositoryError::StorageBackendNotFound(backend) => {
                DeleteRepositoryErrorResponse::not_found(format!("Storage backend '{}' not found", backend))
            },
            RepositoryError::ReferencedRepositoryNotFound(repo_id) => {
                DeleteRepositoryErrorResponse::not_found(format!("Referenced repository '{}' not found", repo_id))
            },
            RepositoryError::RepositoryAlreadyExists(name) => {
                DeleteRepositoryErrorResponse::conflict(
                    format!("Repository '{}' already exists", name)
                )
            },
            RepositoryError::RepositoryTypeMismatch { expected, actual } => {
                DeleteRepositoryErrorResponse::invalid_request(
                    format!("Repository type mismatch: expected {}, got {}", expected, actual)
                )
            },
            RepositoryError::InvalidConfiguration(message) => {
                DeleteRepositoryErrorResponse::invalid_request(format!("Invalid configuration: {}", message))
            },
            RepositoryError::RepositoryNotEmpty(repo_id) => {
                DeleteRepositoryErrorResponse::not_empty(
                    format!("Repository '{}' is not empty", repo_id)
                )
            },
        }
    }
}

/// Handler de Axum para el endpoint de eliminación de repositorio
pub async fn delete_repository_handler(
    Extension(endpoint): Extension<Arc<DeleteRepositoryEndpoint>>,
    Extension(user_id): Extension<UserId>,
    Path(repository_hrn): Path<String>,
    Query(query): Query<DeleteRepositoryQuery>,
) -> impl IntoResponse {
    match endpoint.delete_repository(repository_hrn, query, user_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error_response) => {
            let status_code = match error_response.error.as_str() {
                "NotFound" => StatusCode::NOT_FOUND,
                "Unauthorized" => StatusCode::FORBIDDEN,
                "InvalidRequest" => StatusCode::BAD_REQUEST,
                "Conflict" => StatusCode::CONFLICT,
                "RepositoryNotEmpty" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status_code, Json(error_response)).into_response()
        }
    }
}