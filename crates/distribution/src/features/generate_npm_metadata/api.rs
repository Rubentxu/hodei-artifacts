// crates/distribution/src/features/generate_npm_metadata/api.rs

//! API para generar metadatos npm

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use super::dto::{GenerateNpmMetadataRequest, GenerateNpmMetadataResponse, GenerateNpmMetadataError};
use super::use_case::GenerateNpmMetadataUseCase;

/// Estado compartido para el endpoint de generación de metadatos npm
pub struct GenerateNpmMetadataEndpoint {
    use_case: GenerateNpmMetadataUseCase,
}

impl GenerateNpmMetadataEndpoint {
    /// Crea una nueva instancia del endpoint
    pub fn new(use_case: GenerateNpmMetadataUseCase) -> Self {
        Self { use_case }
    }

    /// Genera metadatos npm para un paquete específico
    #[instrument(
        skip(self, scope, package_name, repository_id),
        fields(
            scope = %scope.as_deref().unwrap_or("none"),
            package_name = %package_name,
            repository_id = %repository_id
        )
    )]
    pub async fn generate_metadata(
        &self,
        scope: Option<String>,
        package_name: String,
        repository_id: String,
        force_regenerate: bool,
    ) -> Result<GenerateNpmMetadataResponse, GenerateNpmMetadataError> {
        info!("Generating npm metadata for package: {}", package_name);

        let request = GenerateNpmMetadataRequest {
            scope,
            package_name,
            repository_id,
            force_regenerate,
        };

        self.use_case.execute(request).await
    }
}

/// Parámetros de la ruta para generar metadatos npm
#[derive(Debug, Deserialize)]
pub struct GenerateMetadataParams {
    /// Scope del paquete npm (opcional, ej: @myorg)
    #[serde(default)]
    pub scope: Option<String>,
    /// Nombre del paquete
    pub package_name: String,
    /// ID del repositorio
    pub repository_id: String,
}

/// Query parameters para la generación de metadatos
#[derive(Debug, Deserialize)]
pub struct GenerateMetadataQuery {
    /// Forzar regeneración de metadatos (ignorar caché)
    #[serde(default)]
    pub force_regenerate: bool,
}

/// Respuesta de error para el API
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl IntoResponse for GenerateNpmMetadataError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_type) = match &self {
            GenerateNpmMetadataError::InvalidPackageName { .. } => {
                (StatusCode::BAD_REQUEST, "invalid_package_name")
            }
            GenerateNpmMetadataError::PackageNotFound { .. } => {
                (StatusCode::NOT_FOUND, "package_not_found")
            }
            GenerateNpmMetadataError::RepositoryError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "repository_error")
            }
            GenerateNpmMetadataError::MetadataGenerationFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "metadata_generation_failed")
            }
            GenerateNpmMetadataError::CacheError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "cache_error")
            }
        };

        let message = self.to_string();
        let details = match &self {
            GenerateNpmMetadataError::InvalidPackageName { name } => {
                Some(serde_json::json!({ "package_name": name }))
            }
            GenerateNpmMetadataError::PackageNotFound { package_name, repository_id } => {
                Some(serde_json::json!({ 
                    "package_name": package_name,
                    "repository_id": repository_id 
                }))
            }
            GenerateNpmMetadataError::RepositoryError(msg) => {
                Some(serde_json::json!({ "error": msg }))
            }
            GenerateNpmMetadataError::MetadataGenerationFailed { reason } => {
                Some(serde_json::json!({ "reason": reason }))
            }
            GenerateNpmMetadataError::CacheError(msg) => {
                Some(serde_json::json!({ "error": msg }))
            }
        };

        let body = Json(ErrorResponse {
            error: error_type.to_string(),
            message,
            details,
        });

        (status, body).into_response()
    }
}

/// Handler para generar metadatos npm
#[instrument(
    skip(endpoint, params, query),
    fields(
        scope = %params.scope.as_deref().unwrap_or("none"),
        package_name = %params.package_name,
        repository_id = %params.repository_id,
        force_regenerate = query.force_regenerate
    )
)]
pub async fn generate_npm_metadata_handler(
    State(endpoint): State<Arc<GenerateNpmMetadataEndpoint>>,
    Path(params): Path<GenerateMetadataParams>,
    axum::extract::Query(query): axum::extract::Query<GenerateMetadataQuery>,
) -> impl IntoResponse {
    info!(
        "Received request to generate npm metadata for package: {} in repository: {}",
        params.package_name, params.repository_id
    );

    match endpoint
        .generate_metadata(
            params.scope,
            params.package_name,
            params.repository_id,
            query.force_regenerate,
        )
        .await
    {
        Ok(response) => {
            info!("Successfully generated npm metadata");
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(error) => {
            error!("Error generating npm metadata: {}", error);
            error.into_response()
        }
    }
}

/// Handler para obtener metadatos npm (alias para generate_npm_metadata_handler)
#[instrument(
    skip(endpoint, params, query),
    fields(
        scope = %params.scope.as_deref().unwrap_or("none"),
        package_name = %params.package_name,
        repository_id = %params.repository_id,
        force_regenerate = query.force_regenerate
    )
)]
pub async fn get_npm_metadata_handler(
    State(endpoint): State<Arc<GenerateNpmMetadataEndpoint>>,
    Path(params): Path<GenerateMetadataParams>,
    axum::extract::Query(query): axum::extract::Query<GenerateMetadataQuery>,
) -> impl IntoResponse {
    // Reutilizar el mismo handler ya que la lógica es idéntica
    generate_npm_metadata_handler(State(endpoint), Path(params), axum::extract::Query(query)).await
}

/// Handler para regenerar metadatos npm (siempre fuerza regeneración)
#[instrument(
    skip(endpoint, params),
    fields(
        scope = %params.scope.as_deref().unwrap_or("none"),
        package_name = %params.package_name,
        repository_id = %params.repository_id
    )
)]
pub async fn regenerate_npm_metadata_handler(
    State(endpoint): State<Arc<GenerateNpmMetadataEndpoint>>,
    Path(params): Path<GenerateMetadataParams>,
) -> impl IntoResponse {
    info!(
        "Received request to regenerate npm metadata for package: {} in repository: {}",
        params.package_name, params.repository_id
    );

    match endpoint
        .generate_metadata(
            params.scope,
            params.package_name,
            params.repository_id,
            true, // Siempre forzar regeneración
        )
        .await
    {
        Ok(response) => {
            info!("Successfully regenerated npm metadata");
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(error) => {
            error!("Error regenerating npm metadata: {}", error);
            error.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generate_npm_metadata::ports::test::{
        MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache,
    };

    fn create_test_endpoint() -> GenerateNpmMetadataEndpoint {
        let metadata_generator = MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: true,
        };
        let package_lister = MockNpmPackageLister::new();
        let metadata_cache = MockNpmMetadataCache::new();

        let use_case = GenerateNpmMetadataUseCase::new(
            Arc::new(metadata_generator),
            Arc::new(package_lister),
            Arc::new(metadata_cache),
            3600, // 1 hour TTL
        );

        GenerateNpmMetadataEndpoint::new(use_case)
    }

    #[tokio::test]
    async fn test_generate_metadata_success() {
        let endpoint = create_test_endpoint();

        let result = endpoint
            .generate_metadata(
                None,
                "test-package".to_string(),
                "test-repo".to_string(),
                false,
            )
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.metadata.name, "test-package");
        assert_eq!(response.metadata.version, "1.0.0");
        assert!(!response.cache_hit);
    }

    #[tokio::test]
    async fn test_generate_metadata_with_scope() {
        let endpoint = create_test_endpoint();

        let result = endpoint
            .generate_metadata(
                Some("@myorg".to_string()),
                "test-package".to_string(),
                "test-repo".to_string(),
                false,
            )
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.metadata.name, "@myorg/test-package");
    }

    #[tokio::test]
    async fn test_generate_metadata_force_regenerate() {
        let endpoint = create_test_endpoint();

        let result = endpoint
            .generate_metadata(
                None,
                "test-package".to_string(),
                "test-repo".to_string(),
                true,
            )
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.cache_hit); // Should not hit cache when force regenerate
    }

    #[tokio::test]
    async fn test_generate_metadata_invalid_package_name() {
        let endpoint = create_test_endpoint();

        let result = endpoint
            .generate_metadata(
                None,
                "".to_string(), // Invalid name
                "test-repo".to_string(),
                false,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GenerateNpmMetadataError::InvalidPackageName { name } => {
                assert_eq!(name, "");
            }
            _ => panic!("Expected InvalidPackageName error"),
        }
    }

    #[tokio::test]
    async fn test_generate_metadata_package_not_found() {
        let metadata_generator = MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: false, // Package doesn't exist
        };
        let package_lister = MockNpmPackageLister::new();
        let metadata_cache = MockNpmMetadataCache::new();

        let use_case = GenerateNpmMetadataUseCase::new(
            Arc::new(metadata_generator),
            Arc::new(package_lister),
            Arc::new(metadata_cache),
            3600,
        );

        let endpoint = GenerateNpmMetadataEndpoint::new(use_case);

        let result = endpoint
            .generate_metadata(
                None,
                "non-existent".to_string(),
                "test-repo".to_string(),
                false,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GenerateNpmMetadataError::PackageNotFound { package_name, repository_id } => {
                assert_eq!(package_name, "non-existent");
                assert_eq!(repository_id, "test-repo");
            }
            _ => panic!("Expected PackageNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_error_response_mapping() {
        let error = GenerateNpmMetadataError::InvalidPackageName {
            name: "invalid-name".to_string(),
        };

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_error_response_with_details() {
        let error = GenerateNpmMetadataError::PackageNotFound {
            package_name: "test-package".to_string(),
            repository_id: "test-repo".to_string(),
        };

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}