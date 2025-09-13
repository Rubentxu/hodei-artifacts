// crates/repository/src/features/update_repository/api.rs

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error, instrument};

use shared::hrn::{RepositoryId, UserId};
use crate::domain::RepositoryResult;

use super::dto::{UpdateRepositoryCommand, UpdateRepositoryResponse};
use super::use_case::UpdateRepositoryUseCase;

/// Request DTO para el endpoint de actualización
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRepositoryRequest {
    /// Nueva descripción (opcional)
    pub description: Option<String>,
    
    /// Nueva configuración (opcional)
    pub config: Option<super::dto::RepositoryConfigUpdateDto>,
    
    /// Nuevo backend de almacenamiento (opcional)
    pub storage_backend_hrn: Option<String>,
    
    /// Nuevo estado de público/privado (opcional)
    pub is_public: Option<bool>,
    
    /// Nuevos metadatos (opcional)
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// Response DTO para errores del endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRepositoryErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<String>,
}

impl UpdateRepositoryErrorResponse {
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
}

/// Punto de entrada de la API para actualizar repositorios
pub struct UpdateRepositoryEndpoint {
    pub use_case: Arc<UpdateRepositoryUseCase>,
}

impl UpdateRepositoryEndpoint {
    pub fn new(use_case: Arc<UpdateRepositoryUseCase>) -> Self {
        Self { use_case }
    }

    #[instrument(skip(self, repository_hrn, request, user_id))]
    pub async fn update_repository(
        &self,
        repository_hrn: String,
        request: UpdateRepositoryRequest,
        user_id: UserId,
    ) -> Result<UpdateRepositoryResponse, UpdateRepositoryErrorResponse> {
        info!("Updating repository with HRN: {}", repository_hrn);

        // Construir el comando
        let command = UpdateRepositoryCommand {
            repository_hrn,
            description: request.description,
            config: request.config,
            storage_backend_hrn: request.storage_backend_hrn,
            is_public: request.is_public,
            metadata: request.metadata,
        };

        // Ejecutar el caso de uso
        match self.use_case.execute(command, user_id).await {
            Ok(response) => {
                info!("Successfully updated repository: {}", response.hrn);
                Ok(response)
            },
            Err(error) => {
                error!("Failed to update repository: {}", error);
                Err(Self::map_error_to_response(error))
            }
        }
    }

    fn map_error_to_response(error: crate::domain::RepositoryError) -> UpdateRepositoryErrorResponse {
        use crate::domain::RepositoryError;

        match error {
            RepositoryError::RepositoryNotFound(repo_id) => {
                UpdateRepositoryErrorResponse::not_found(format!("Repository '{}' not found", repo_id))
            },
            RepositoryError::Unauthorized(message) => {
                UpdateRepositoryErrorResponse::unauthorized(message)
            },
            RepositoryError::InvalidRepositoryName(message) => {
                UpdateRepositoryErrorResponse::invalid_request(format!("Invalid repository name: {}", message))
            },
            _ => UpdateRepositoryErrorResponse::internal_error("An unexpected error occurred".to_string(), Some(error.to_string())),
        }
    }
}

/// Handler de Axum para el endpoint de actualización de repositorio
pub async fn update_repository_handler(
    Extension(endpoint): Extension<Arc<UpdateRepositoryEndpoint>>,
    Extension(user_id): Extension<UserId>,
    Path(repository_hrn): Path<String>,
    Json(request): Json<UpdateRepositoryRequest>,
) -> impl IntoResponse {
    match endpoint.update_repository(repository_hrn, request, user_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error_response) => {
            let status_code = match error_response.error.as_str() {
                "NotFound" => StatusCode::NOT_FOUND,
                "Unauthorized" => StatusCode::FORBIDDEN,
                "InvalidRequest" => StatusCode::BAD_REQUEST,
                "Conflict" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status_code, Json(error_response)).into_response()
        }
    }
}

/// Handler alternativo usando PATCH
pub async fn patch_repository_handler(
    Extension(endpoint): Extension<Arc<UpdateRepositoryEndpoint>>,
    Extension(user_id): Extension<UserId>,
    Path(repository_hrn): Path<String>,
    Json(request): Json<UpdateRepositoryRequest>,
) -> impl IntoResponse {
    // PATCH es idéntico a PUT en esta implementación
    update_repository_handler(Extension(endpoint), Extension(user_id), Path(repository_hrn), Json(request)).await
}