// crates/repository/src/features/get_repository/api.rs

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

use super::dto::{GetRepositoryQuery, GetRepositoryResponse};
use super::use_case::GetRepositoryUseCase;

/// Request DTO para el endpoint de obtención de repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRepositoryRequest {
    /// HRN del repositorio a obtener
    pub repository_hrn: String,
}

/// Response DTO para errores del endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRepositoryErrorResponse {
    pub error: String,
    pub message: String,
    pub details: Option<String>,
}

impl GetRepositoryErrorResponse {
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
}

/// Punto de entrada de la API para obtener repositorios
pub struct GetRepositoryEndpoint {
    pub use_case: Arc<GetRepositoryUseCase>,
}

impl GetRepositoryEndpoint {
    pub fn new(use_case: Arc<GetRepositoryUseCase>) -> Self {
        Self { use_case }
    }

    #[instrument(skip(self, repository_hrn, user_id))]
    pub async fn get_repository(
        &self,
        repository_hrn: String,
        user_id: UserId,
    ) -> Result<GetRepositoryResponse, GetRepositoryErrorResponse> {
        info!("Getting repository with HRN: {}", repository_hrn);

        // Construir la query
        let query = GetRepositoryQuery {
            repository_hrn: repository_hrn.clone(),
        };

        // Ejecutar el caso de uso
        match self.use_case.execute(query, user_id).await {
            Ok(response) => {
                info!("Successfully retrieved repository: {}", repository_hrn);
                Ok(response)
            },
            Err(error) => {
                error!("Failed to get repository {}: {}", repository_hrn, error);
                Err(Self::map_error_to_response(error))
            }
        }
    }

    fn map_error_to_response(error: crate::domain::RepositoryError) -> GetRepositoryErrorResponse {
        use crate::domain::RepositoryError;

        match error {
            RepositoryError::RepositoryNotFound(repo_id) => {
                GetRepositoryErrorResponse::not_found(format!("Repository '{}' not found", repo_id))
            },
            RepositoryError::Unauthorized(message) => {
                GetRepositoryErrorResponse::unauthorized(message)
            },
            RepositoryError::InvalidRepositoryName(message) => {
                GetRepositoryErrorResponse::invalid_request(format!("Invalid repository name: {}", message))
            },
            _ => GetRepositoryErrorResponse::internal_error("An unexpected error occurred".to_string(), Some(error.to_string())),
        }
    }
}

/// Handler de Axum para el endpoint de obtención de repositorio
pub async fn get_repository_handler(
    Extension(endpoint): Extension<Arc<GetRepositoryEndpoint>>,
    Extension(user_id): Extension<UserId>,
    Path(repository_hrn): Path<String>,
) -> impl IntoResponse {
    match endpoint.get_repository(repository_hrn, user_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error_response) => {
            let status_code = match error_response.error.as_str() {
                "NotFound" => StatusCode::NOT_FOUND,
                "Unauthorized" => StatusCode::FORBIDDEN,
                "InvalidRequest" => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status_code, Json(error_response)).into_response()
        }
    }
}

/// Handler alternativo usando query parameters
pub async fn get_repository_by_query_handler(
    Extension(endpoint): Extension<Arc<GetRepositoryEndpoint>>,
    Extension(user_id): Extension<UserId>,
    axum::extract::Query(request): axum::extract::Query<GetRepositoryRequest>,
) -> impl IntoResponse {
    match endpoint.get_repository(request.repository_hrn, user_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error_response) => {
            let status_code = match error_response.error.as_str() {
                "NotFound" => StatusCode::NOT_FOUND,
                "Unauthorized" => StatusCode::FORBIDDEN,
                "InvalidRequest" => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status_code, Json(error_response)).into_response()
        }
    }
}