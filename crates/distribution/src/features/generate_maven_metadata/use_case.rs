// crates/distribution/src/features/generate_maven_metadata/use_case.rs

//! Caso de uso para generar Maven metadata

use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use crate::domain::maven::{MavenCoordinates, MavenVersion};
use super::dto::{GenerateMavenMetadataRequest, GenerateMavenMetadataResponse, MavenMetadataDto};
use super::ports::{MavenMetadataGenerator, MavenArtifactLister, MavenMetadataCache};

/// Caso de uso para generar Maven metadata
pub struct GenerateMavenMetadataUseCase {
    metadata_generator: Arc<dyn MavenMetadataGenerator>,
    artifact_lister: Arc<dyn MavenArtifactLister>,
    metadata_cache: Arc<dyn MavenMetadataCache>,
}

impl GenerateMavenMetadataUseCase {
    /// Crear una nueva instancia del caso de uso
    pub fn new(
        metadata_generator: Arc<dyn MavenMetadataGenerator>,
        artifact_lister: Arc<dyn MavenArtifactLister>,
        metadata_cache: Arc<dyn MavenMetadataCache>,
    ) -> Self {
        Self {
            metadata_generator,
            artifact_lister,
            metadata_cache,
        }
    }
    
    /// Ejecutar el caso de uso para generar Maven metadata
    #[instrument(
        name = "generate_maven_metadata",
        skip(self, request),
        fields(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id
        )
    )]
    pub async fn execute(
        &self,
        request: GenerateMavenMetadataRequest,
    ) -> Result<GenerateMavenMetadataResponse, super::dto::MavenMetadataError> {
        info!(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id,
            "Generating Maven metadata"
        );
        
        // Verificar si hay metadata en caché
        if let Some(cached_metadata) = self.metadata_cache
            .get_cached_metadata(&request.coordinates, &request.repository_id)
            .await?
        {
            info!("Returning cached Maven metadata");
            return Ok(GenerateMavenMetadataResponse {
                metadata: cached_metadata,
                from_cache: true,
            });
        }
        
        // Listar todas las versiones del artefacto
        let versions = self.artifact_lister
            .list_versions(&request.coordinates, &request.repository_id)
            .await?;
        
        if versions.is_empty() {
            error!("No versions found for artifact");
            return Err(super::dto::MavenMetadataError::ArtifactNotFound {
                coordinates: request.coordinates.to_string(),
            });
        }
        
        // Generar metadata
        let metadata = self.metadata_generator
            .generate_metadata(&request.coordinates, &request.repository_id)
            .await?;
        
        // Cachear la metadata generada
        if let Err(e) = self.metadata_cache
            .cache_metadata(&request.coordinates, &request.repository_id, &metadata)
            .await
        {
            warn!("Failed to cache metadata: {}", e);
        }
        
        info!(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id,
            versions_count = versions.len(),
            "Successfully generated Maven metadata"
        );
        
        Ok(GenerateMavenMetadataResponse {
            metadata,
            from_cache: false,
        })
    }
    
    /// Generar metadata XML para Maven
    #[instrument(
        name = "generate_maven_metadata_xml",
        skip(self, request),
        fields(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id
        )
    )]
    pub async fn generate_xml(
        &self,
        request: GenerateMavenMetadataRequest,
    ) -> Result<String, super::dto::MavenMetadataError> {
        info!(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id,
            "Generating Maven metadata XML"
        );
        
        // Generar XML metadata
        let xml_content = self.metadata_generator
            .generate_metadata_xml(&request.coordinates, &request.repository_id)
            .await?;
        
        info!(
            repository_id = %request.repository_id,
            group_id = %request.coordinates.group_id,
            artifact_id = %request.coordinates.artifact_id,
            xml_length = xml_content.len(),
            "Successfully generated Maven metadata XML"
        );
        
        Ok(xml_content)
    }
    
    /// Invalidar caché de metadata para un artefacto específico
    #[instrument(
        name = "invalidate_maven_metadata_cache",
        skip(self, coordinates, repository_id),
        fields(
            repository_id = %repository_id,
            group_id = %coordinates.group_id,
            artifact_id = %coordinates.artifact_id
        )
    )]
    pub async fn invalidate_cache(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<(), super::dto::MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            group_id = %coordinates.group_id,
            artifact_id = %coordinates.artifact_id,
            "Invalidating Maven metadata cache"
        );
        
        self.metadata_cache
            .invalidate_cache(coordinates, repository_id)
            .await?;
        
        info!("Successfully invalidated Maven metadata cache");
        Ok(())
    }
    
    /// Invalidar caché de metadata para todo un repositorio
    #[instrument(
        name = "invalidate_maven_repository_cache",
        skip(self, repository_id),
        fields(repository_id = %repository_id)
    )]
    pub async fn invalidate_repository_cache(
        &self,
        repository_id: &str,
    ) -> Result<(), super::dto::MavenMetadataError> {
        info!(
            repository_id = %repository_id,
            "Invalidating Maven metadata cache for repository"
        );
        
        self.metadata_cache
            .invalidate_repository_cache(repository_id)
            .await?;
        
        info!("Successfully invalidated Maven metadata cache for repository");
        Ok(())
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
    async fn test_generate_maven_metadata_success() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        );
        
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
        
        let request = GenerateMavenMetadataRequest {
            coordinates: coordinates.clone(),
            repository_id: "test-repo".to_string(),
        };
        
        let result = use_case.execute(request).await.unwrap();
        
        assert_eq!(result.metadata.group_id, "com.example");
        assert_eq!(result.metadata.artifact_id, "test-artifact");
        assert!(!result.from_cache);
    }
    
    #[tokio::test]
    async fn test_generate_maven_metadata_from_cache() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        );
        
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string()])
            .build();
        
        // Pre-cache metadata
        metadata_cache.cache_metadata(&coordinates, "test-repo", &metadata).await.unwrap();
        
        let request = GenerateMavenMetadataRequest {
            coordinates: coordinates.clone(),
            repository_id: "test-repo".to_string(),
        };
        
        let result = use_case.execute(request).await.unwrap();
        
        assert_eq!(result.metadata.group_id, "com.example");
        assert_eq!(result.metadata.artifact_id, "test-artifact");
        assert!(result.from_cache);
    }
    
    #[tokio::test]
    async fn test_generate_maven_metadata_no_versions() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        );
        
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        
        let request = GenerateMavenMetadataRequest {
            coordinates: coordinates.clone(),
            repository_id: "test-repo".to_string(),
        };
        
        let result = use_case.execute(request).await;
        
        assert!(result.is_err());
        match result {
            Err(super::dto::MavenMetadataError::ArtifactNotFound { .. }) => {},
            _ => panic!("Expected ArtifactNotFound error"),
        }
    }
    
    #[tokio::test]
    async fn test_invalidate_cache() {
        let metadata_generator = Arc::new(MockMavenMetadataGenerator::new());
        let artifact_lister = Arc::new(MockMavenArtifactLister::new());
        let metadata_cache = Arc::new(MockMavenMetadataCache::new());
        
        let use_case = GenerateMavenMetadataUseCase::new(
            metadata_generator.clone(),
            artifact_lister.clone(),
            metadata_cache.clone(),
        );
        
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
        use_case.invalidate_cache(&coordinates, "test-repo").await.unwrap();
        
        // Verify it's no longer cached
        let cached_after = metadata_cache.get_cached_metadata(&coordinates, "test-repo").await.unwrap();
        assert!(cached_after.is_none());
    }
}

// Helper builder para MavenMetadataDto
pub struct MavenMetadataDtoBuilder {
    group_id: String,
    artifact_id: String,
    versions: Vec<String>,
}

impl MavenMetadataDtoBuilder {
    pub fn new(group_id: String, artifact_id: String) -> Self {
        Self {
            group_id,
            artifact_id,
            versions: Vec::new(),
        }
    }
    
    pub fn versions(mut self, versions: Vec<String>) -> Self {
        self.versions = versions;
        self
    }
    
    pub fn build(self) -> MavenMetadataDto {
        let latest = self.versions.last().cloned().unwrap_or_default();
        let release = self.versions.iter()
            .filter(|v| !v.contains("-SNAPSHOT"))
            .last()
            .cloned()
            .unwrap_or_else(|| latest.clone());
        
        MavenMetadataDto {
            group_id: self.group_id,
            artifact_id: self.artifact_id,
            versioning: super::dto::MavenMetadataVersioningDto {
                latest,
                release,
                versions: self.versions,
                last_updated: chrono::Utc::now().to_rfc3339(),
                snapshot: None,
            },
        }
    }
}