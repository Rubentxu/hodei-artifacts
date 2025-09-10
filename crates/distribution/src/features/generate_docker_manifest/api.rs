// crates/distribution/src/features/generate_docker_manifest/api.rs

//! API endpoints para la generación de manifests Docker

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tracing::{info, error, instrument};
use std::sync::Arc;

use crate::features::generate_docker_manifest::{
    dto::{GenerateDockerManifestRequest, GenerateDockerManifestResponse, DockerManifestError},
    use_case::GenerateDockerManifestUseCase,
};

/// Handler de API para la generación de manifests Docker
pub struct GenerateDockerManifestApi {
    use_case: Arc<GenerateDockerManifestUseCase>,
}

impl GenerateDockerManifestApi {
    pub fn new(use_case: Arc<GenerateDockerManifestUseCase>) -> Self {
        Self { use_case }
    }

    /// Generar manifest Docker para un repositorio y tag específicos
    #[instrument(skip(self, request))]
    pub async fn generate_manifest(
        &self,
        request: GenerateDockerManifestRequest,
    ) -> Result<GenerateDockerManifestResponse, DockerManifestError> {
        info!(
            repository_name = %request.repository_name,
            tag = %request.tag,
            "Generando manifest Docker"
        );

        let response = self.use_case.execute(request).await?;

        info!(
            repository_name = %response.repository_name,
            tag = %response.tag,
            manifest_media_type = %response.manifest.media_type,
            "Manifest Docker generado exitosamente"
        );

        Ok(response)
    }

    /// Handler HTTP para POST /v2/{name}/manifests/{tag}
    #[instrument(skip(self))]
    pub async fn handle_generate_manifest(
        State(api): State<Arc<Self>>,
        Path((repository_name, tag)): Path<(String, String)>,
        Json(payload): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let request = GenerateDockerManifestRequest {
            repository_name,
            tag,
            regenerate: payload.get("regenerate")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };

        match self.generate_manifest(request).await {
            Ok(response) => {
                let json_response = json!({
                    "repository_name": response.repository_name,
                    "tag": response.tag,
                    "manifest": response.manifest,
                    "digest": response.digest,
                    "size": response.size,
                    "media_type": response.media_type,
                    "schema_version": response.schema_version,
                    "architecture": response.architecture,
                    "os": response.os,
                    "layers": response.layers,
                    "config": response.config,
                });

                (StatusCode::OK, Json(json_response)).into_response()
            }
            Err(error) => {
                error!(error = %error, "Error al generar manifest Docker");
                error.into_response()
            }
        }
    }

    /// Handler HTTP para GET /v2/{name}/manifests/{tag}/generate
    #[instrument(skip(self))]
    pub async fn handle_get_generated_manifest(
        State(api): State<Arc<Self>>,
        Path((repository_name, tag)): Path<(String, String)>,
    ) -> impl IntoResponse {
        let request = GenerateDockerManifestRequest {
            repository_name,
            tag,
            regenerate: false,
        };

        match self.generate_manifest(request).await {
            Ok(response) => {
                let json_response = json!({
                    "repository_name": response.repository_name,
                    "tag": response.tag,
                    "manifest": response.manifest,
                    "digest": response.digest,
                    "size": response.size,
                    "media_type": response.media_type,
                    "schema_version": response.schema_version,
                    "architecture": response.architecture,
                    "os": response.os,
                    "layers": response.layers,
                    "config": response.config,
                });

                (StatusCode::OK, Json(json_response)).into_response()
            }
            Err(error) => {
                error!(error = %error, "Error al obtener manifest Docker generado");
                error.into_response()
            }
        }
    }
}

impl IntoResponse for DockerManifestError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            DockerManifestError::InvalidRepositoryName(name) => {
                (StatusCode::BAD_REQUEST, format!("Invalid repository name: {}", name))
            }
            DockerManifestError::InvalidTag(tag) => {
                (StatusCode::BAD_REQUEST, format!("Invalid tag: {}", tag))
            }
            DockerManifestError::RepositoryNotFound(name) => {
                (StatusCode::NOT_FOUND, format!("Repository not found: {}", name))
            }
            DockerManifestError::TagNotFound { repository, tag } => {
                (StatusCode::NOT_FOUND, format!("Tag {} not found in repository {}", tag, repository))
            }
            DockerManifestError::PermissionDenied { repository, user } => {
                (StatusCode::FORBIDDEN, format!("Permission denied for repository {} for user {}", repository, user))
            }
            DockerManifestError::ManifestGenerationFailed { repository, tag, reason } => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to generate manifest for {}/{}: {}", repository, tag, reason))
            }
            DockerManifestError::LayerListingFailed { repository, reason } => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to list layers for repository {}: {}", repository, reason))
            }
            DockerManifestError::CacheError { reason } => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Cache error: {}", reason))
            }
            DockerManifestError::InternalError { reason } => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Internal error: {}", reason))
            }
        };

        let body = Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": error_message,
                "type": "docker_manifest_error",
            }
        }));

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generate_docker_manifest::{
        dto::{DockerManifestDto, DockerDescriptorDto},
        ports::{MockDockerManifestGenerator, MockDockerLayerLister, MockDockerManifestCache},
        use_case::GenerateDockerManifestUseCase,
    };
    use std::sync::Arc;

    fn create_test_api() -> GenerateDockerManifestApi {
        let generator = Arc::new(MockDockerManifestGenerator::new());
        let layer_lister = Arc::new(MockDockerLayerLister::new());
        let cache = Arc::new(MockDockerManifestCache::new());
        
        let use_case = Arc::new(GenerateDockerManifestUseCase::new(
            generator,
            layer_lister,
            cache,
        ));

        GenerateDockerManifestApi::new(use_case)
    }

    #[tokio::test]
    async fn test_generate_manifest_success() {
        let api = create_test_api();
        
        let request = GenerateDockerManifestRequest {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            regenerate: false,
        };

        let result = api.generate_manifest(request).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.repository_name, "test/repo");
        assert_eq!(response.tag, "latest");
        assert_eq!(response.schema_version, 2);
    }

    #[tokio::test]
    async fn test_generate_manifest_invalid_repository() {
        let api = create_test_api();
        
        let request = GenerateDockerManifestRequest {
            repository_name: "invalid/repo/name".to_string(),
            tag: "latest".to_string(),
            regenerate: false,
        };

        let result = api.generate_manifest(request).await;
        assert!(result.is_err());
        
        match result.unwrap_err() {
            DockerManifestError::InvalidRepositoryName(name) => {
                assert_eq!(name, "invalid/repo/name");
            }
            _ => panic!("Expected InvalidRepositoryName error"),
        }
    }

    #[tokio::test]
    async fn test_generate_manifest_invalid_tag() {
        let api = create_test_api();
        
        let request = GenerateDockerManifestRequest {
            repository_name: "test/repo".to_string(),
            tag: "invalid@tag".to_string(),
            regenerate: false,
        };

        let result = api.generate_manifest(request).await;
        assert!(result.is_err());
        
        match result.unwrap_err() {
            DockerManifestError::InvalidTag(tag) => {
                assert_eq!(tag, "invalid@tag");
            }
            _ => panic!("Expected InvalidTag error"),
        }
    }
}