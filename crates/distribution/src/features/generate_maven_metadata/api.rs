// crates/distribution/src/features/generate_maven_metadata/api.rs

//! API para generar Maven metadata

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use crate::domain::maven::MavenCoordinates;
use super::dto::{GenerateMavenMetadataRequest, GenerateMavenMetadataResponse, MavenMetadataError};
use super::use_case::GenerateMavenMetadataUseCase;

/// API para generar Maven metadata
pub struct GenerateMavenMetadataApi {
    use_case: Arc<GenerateMavenMetadataUseCase>,
}

impl GenerateMavenMetadataApi {
    /// Crear una nueva instancia del API
    pub fn new(use_case: Arc<GenerateMavenMetadataUseCase>) -> Self {
        Self { use_case }
    }
    
    /// Generar Maven metadata para un artefacto específico
    #[instrument(
        name = "generate_maven_metadata_api",
        skip(self, repository_id, group_id, artifact_id),
        fields(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id
        )
    )]
    pub async fn generate_metadata(
        &self,
        repository_id: String,
        group_id: String,
        artifact_id: String,
    ) -> Result<Json<GenerateMavenMetadataResponse>, MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id,
            "Generating Maven metadata via API"
        );
        
        let coordinates = MavenCoordinates::new(&group_id, &artifact_id, "1.0.0")
            .map_err(|e| MavenMetadataError::InvalidCoordinates {
                coordinates: format!("{}:{}", group_id, artifact_id),
                reason: e.to_string(),
            })?;
        
        let request = GenerateMavenMetadataRequest {
            coordinates,
            repository_id,
        };
        
        let response = self.use_case.execute(request).await?;
        
        info!(
            repository_id = %response.metadata.group_id,
            group_id = %response.metadata.group_id,
            artifact_id = %response.metadata.artifact_id,
            from_cache = response.from_cache,
            "Successfully generated Maven metadata via API"
        );
        
        Ok(Json(response))
    }
    
    /// Generar Maven metadata XML para un artefacto específico
    #[instrument(
        name = "generate_maven_metadata_xml_api",
        skip(self, repository_id, group_id, artifact_id),
        fields(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id
        )
    )]
    pub async fn generate_metadata_xml(
        &self,
        repository_id: String,
        group_id: String,
        artifact_id: String,
    ) -> Result<String, MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id,
            "Generating Maven metadata XML via API"
        );
        
        let coordinates = MavenCoordinates::new(&group_id, &artifact_id, "1.0.0")
            .map_err(|e| MavenMetadataError::InvalidCoordinates {
                coordinates: format!("{}:{}", group_id, artifact_id),
                reason: e.to_string(),
            })?;
        
        let request = GenerateMavenMetadataRequest {
            coordinates,
            repository_id,
        };
        
        let xml_content = self.use_case.generate_xml(request).await?;
        
        info!(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id,
            xml_length = xml_content.len(),
            "Successfully generated Maven metadata XML via API"
        );
        
        Ok(xml_content)
    }
    
    /// Invalidar caché de metadata para un artefacto específico
    #[instrument(
        name = "invalidate_maven_metadata_cache_api",
        skip(self, repository_id, group_id, artifact_id),
        fields(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id
        )
    )]
    pub async fn invalidate_cache(
        &self,
        repository_id: String,
        group_id: String,
        artifact_id: String,
    ) -> Result<StatusCode, MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id,
            "Invalidating Maven metadata cache via API"
        );
        
        let coordinates = MavenCoordinates::new(&group_id, &artifact_id, "1.0.0")
            .map_err(|e| MavenMetadataError::InvalidCoordinates {
                coordinates: format!("{}:{}", group_id, artifact_id),
                reason: e.to_string(),
            })?;
        
        self.use_case.invalidate_cache(&coordinates, &repository_id).await?;
        
        info!(
            repository_id = %repository_id,
            group_id = %group_id,
            artifact_id = %artifact_id,
            "Successfully invalidated Maven metadata cache via API"
        );
        
        Ok(StatusCode::NO_CONTENT)
    }
    
    /// Invalidar caché de metadata para todo un repositorio
    #[instrument(
        name = "invalidate_maven_repository_cache_api",
        skip(self, repository_id),
        fields(repository_id = %repository_id)
    )]
    pub async fn invalidate_repository_cache(
        &self,
        repository_id: String,
    ) -> Result<StatusCode, MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            "Invalidating Maven metadata cache for repository via API"
        );
        
        self.use_case.invalidate_repository_cache(&repository_id).await?;
        
        info!(
            repository_id = %repository_id,
            "Successfully invalidated Maven metadata cache for repository via API"
        );
        
        Ok(StatusCode::NO_CONTENT)
    }
}

/// Handler de Axum para generar Maven metadata
pub async fn generate_metadata_handler(
    Path((repository_id, group_id, artifact_id)): Path<(String, String, String)>,
    State(api): State<Arc<GenerateMavenMetadataApi>>,
) -> impl IntoResponse {
    match api.generate_metadata(repository_id, group_id, artifact_id).await {
        Ok(response) => (StatusCode::OK, response).into_response(),
        Err(error) => {
            error!("Error generating Maven metadata: {}", error);
            match error {
                MavenMetadataError::ArtifactNotFound { .. } => {
                    (StatusCode::NOT_FOUND, Json(serde_json::json!({
                        "error": "Artifact not found",
                        "message": error.to_string()
                    }))).into_response()
                },
                MavenMetadataError::InvalidCoordinates { .. } => {
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                        "error": "Invalid coordinates",
                        "message": error.to_string()
                    }))).into_response()
                },
                MavenMetadataError::CacheError { .. } => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Cache error",
                        "message": error.to_string()
                    }))).into_response()
                },
                _ => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Internal server error",
                        "message": error.to_string()
                    }))).into_response()
                },
            }
        }
    }
}

/// Handler de Axum para generar Maven metadata XML
pub async fn generate_metadata_xml_handler(
    Path((repository_id, group_id, artifact_id)): Path<(String, String, String)>,
    State(api): State<Arc<GenerateMavenMetadataApi>>,
) -> impl IntoResponse {
    match api.generate_metadata_xml(repository_id, group_id, artifact_id).await {
        Ok(xml_content) => {
            (
                StatusCode::OK,
                [("Content-Type", "application/xml")],
                xml_content,
            ).into_response()
        },
        Err(error) => {
            error!("Error generating Maven metadata XML: {}", error);
            match error {
                MavenMetadataError::ArtifactNotFound { .. } => {
                    (StatusCode::NOT_FOUND, Json(serde_json::json!({
                        "error": "Artifact not found",
                        "message": error.to_string()
                    }))).into_response()
                },
                MavenMetadataError::InvalidCoordinates { .. } => {
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                        "error": "Invalid coordinates",
                        "message": error.to_string()
                    }))).into_response()
                },
                MavenMetadataError::CacheError { .. } => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Cache error",
                        "message": error.to_string()
                    }))).into_response()
                },
                _ => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Internal server error",
                        "message": error.to_string()
                    }))).into_response()
                },
            }
        }
    }
}

/// Handler de Axum para invalidar caché de metadata
pub async fn invalidate_cache_handler(
    Path((repository_id, group_id, artifact_id)): Path<(String, String, String)>,
    State(api): State<Arc<GenerateMavenMetadataApi>>,
) -> impl IntoResponse {
    match api.invalidate_cache(repository_id, group_id, artifact_id).await {
        Ok(status) => status.into_response(),
        Err(error) => {
            error!("Error invalidating Maven metadata cache: {}", error);
            match error {
                MavenMetadataError::InvalidCoordinates { .. } => {
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                        "error": "Invalid coordinates",
                        "message": error.to_string()
                    }))).into_response()
                },
                MavenMetadataError::CacheError { .. } => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Cache error",
                        "message": error.to_string()
                    }))).into_response()
                },
                _ => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Internal server error",
                        "message": error.to_string()
                    }))).into_response()
                },
            }
        }
    }
}

/// Handler de Axum para invalidar caché de metadata de repositorio
pub async fn invalidate_repository_cache_handler(
    Path(repository_id): Path<String>,
    State(api): State<Arc<GenerateMavenMetadataApi>>,
) -> impl IntoResponse {
    match api.invalidate_repository_cache(repository_id).await {
        Ok(status) => status.into_response(),
        Err(error) => {
            error!("Error invalidating Maven metadata repository cache: {}", error);
            match error {
                MavenMetadataError::CacheError { .. } => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Cache error",
                        "message": error.to_string()
                    }))).into_response()
                },
                _ => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "Internal server error",
                        "message": error.to_string()
                    }))).into_response()
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::maven::MavenCoordinates;
    use super::super::ports::test::{
        MockMavenMetadataGenerator, MockMavenArtifactLister, MockMavenMetadataCache,
    };
    
    #[tokio::test]
    async fn test_generate_metadata_api_success() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = Arc::new(GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        ));
        
        let api = Arc::new(GenerateMavenMetadataApi::new(use_case));
        
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string(), "1.1.0".to_string()])
            .build();
        
        // Setup mocks
        artifact_lister.add_versions(
            "test-repo:com.example:test-artifact".to_string(),
            vec![
                MavenVersion::new("1.0.0").unwrap(),
                MavenVersion::new("1.1.0").unwrap(),
            ],
        );
        
        metadata_generator.add_metadata(
            "test-repo:com.example:test-artifact".to_string(),
            metadata.clone(),
        );
        
        let result = api.generate_metadata(
            "test-repo".to_string(),
            "com.example".to_string(),
            "test-artifact".to_string(),
        ).await.unwrap();
        
        assert_eq!(result.metadata.group_id, "com.example");
        assert_eq!(result.metadata.artifact_id, "test-artifact");
        assert!(!result.from_cache);
    }
    
    #[tokio::test]
    async fn test_generate_metadata_xml_api_success() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = Arc::new(GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        ));
        
        let api = Arc::new(GenerateMavenMetadataApi::new(use_case));
        
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>com.example</groupId>
  <artifactId>test-artifact</artifactId>
  <versioning>
    <latest>1.1.0</latest>
    <release>1.1.0</release>
    <versions>
      <version>1.0.0</version>
      <version>1.1.0</version>
    </versions>
    <lastUpdated>20240101120000</lastUpdated>
  </versioning>
</metadata>"#.to_string();
        
        // Setup mocks
        metadata_generator.add_xml_metadata(
            "test-repo:com.example:test-artifact".to_string(),
            xml_content.clone(),
        );
        
        let result = api.generate_metadata_xml(
            "test-repo".to_string(),
            "com.example".to_string(),
            "test-artifact".to_string(),
        ).await.unwrap();
        
        assert!(result.contains("com.example"));
        assert!(result.contains("test-artifact"));
        assert!(result.contains("1.0.0"));
        assert!(result.contains("1.1.0"));
    }
    
    #[tokio::test]
    async fn test_invalidate_cache_api_success() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = Arc::new(GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        ));
        
        let api = Arc::new(GenerateMavenMetadataApi::new(use_case));
        
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string()])
            .build();
        
        // Pre-cache metadata
        metadata_cache.cache_metadata(&coordinates, "test-repo", &metadata).await.unwrap();
        
        // Verify it's cached
        let cached = metadata_cache.get_cached_metadata(&coordinates, "test-repo").await.unwrap();
        assert!(cached.is_some());
        
        // Invalidate cache
        let result = api.invalidate_cache(
            "test-repo".to_string(),
            "com.example".to_string(),
            "test-artifact".to_string(),
        ).await.unwrap();
        
        assert_eq!(result, StatusCode::NO_CONTENT);
        
        // Verify it's no longer cached
        let cached_after = metadata_cache.get_cached_metadata(&coordinates, "test-repo").await.unwrap();
        assert!(cached_after.is_none());
    }
    
    #[tokio::test]
    async fn test_invalidate_repository_cache_api_success() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = Arc::new(GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        ));
        
        let api = Arc::new(GenerateMavenMetadataApi::new(use_case));
        
        let coordinates1 = MavenCoordinates::new("com.example", "test-artifact-1", "1.0.0").unwrap();
        let coordinates2 = MavenCoordinates::new("com.example", "test-artifact-2", "1.0.0").unwrap();
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string()])
            .build();
        
        // Pre-cache metadata for multiple artifacts
        metadata_cache.cache_metadata(&coordinates1, "test-repo", &metadata).await.unwrap();
        metadata_cache.cache_metadata(&coordinates2, "test-repo", &metadata).await.unwrap();
        
        // Verify they're cached
        let cached1 = metadata_cache.get_cached_metadata(&coordinates1, "test-repo").await.unwrap();
        let cached2 = metadata_cache.get_cached_metadata(&coordinates2, "test-repo").await.unwrap();
        assert!(cached1.is_some());
        assert!(cached2.is_some());
        
        // Invalidate repository cache
        let result = api.invalidate_repository_cache("test-repo".to_string()).await.unwrap();
        
        assert_eq!(result, StatusCode::NO_CONTENT);
        
        // Verify they're no longer cached
        let cached1_after = metadata_cache.get_cached_metadata(&coordinates1, "test-repo").await.unwrap();
        let cached2_after = metadata_cache.get_cached_metadata(&coordinates2, "test-repo").await.unwrap();
        assert!(cached1_after.is_none());
        assert!(cached2_after.is_none());
    }
}

// Importar el builder desde el archivo de use_case
use super::use_case::MavenMetadataDtoBuilder;
use crate::domain::maven::MavenVersion;