// crates/distribution/src/features/generate_maven_metadata/ports.rs

//! Puertos segregados para la generación de Maven metadata

use async_trait::async_trait;
use std::sync::Arc;
use crate::domain::maven::{MavenCoordinates, MavenVersion};
use super::dto::{MavenMetadataDto, MavenMetadataError};

/// Puerto para generar Maven metadata
#[async_trait]
pub trait MavenMetadataGenerator: Send + Sync {
    /// Generar metadata para un artefacto Maven
    async fn generate_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<MavenMetadataDto, MavenMetadataGeneratorError>;
    
    /// Generar XML metadata para un artefacto Maven
    async fn generate_metadata_xml(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<String, MavenMetadataGeneratorError>;
}

/// Puerto para listar artefactos Maven
#[async_trait]
pub trait MavenArtifactLister: Send + Sync {
    /// Listar todas las versiones de un artefacto
    async fn list_versions(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<Vec<MavenVersion>, MavenArtifactListerError>;
    
    /// Listar todos los artefactos en un grupo
    async fn list_artifacts_in_group(
        &self,
        group_id: &str,
        repository_id: &str,
    ) -> Result<Vec<String>, MavenArtifactListerError>;
    
    /// Verificar si existe un artefacto
    async fn artifact_exists(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<bool, MavenArtifactListerError>;
}

/// Puerto para caché de metadata
#[async_trait]
pub trait MavenMetadataCache: Send + Sync {
    /// Obtener metadata desde caché
    async fn get_cached_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<Option<MavenMetadataDto>, MavenMetadataCacheError>;
    
    /// Guardar metadata en caché
    async fn cache_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
        metadata: &MavenMetadataDto,
    ) -> Result<(), MavenMetadataCacheError>;
    
    /// Invalidar caché para un artefacto
    async fn invalidate_cache(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<(), MavenMetadataCacheError>;
    
    /// Invalidar caché para todo un repositorio
    async fn invalidate_repository_cache(
        &self,
        repository_id: &str,
    ) -> Result<(), MavenMetadataCacheError>;
}

/// Error de generación de metadata
#[derive(Debug, thiserror::Error)]
pub enum MavenMetadataGeneratorError {
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Artifact not found: {coordinates}")]
    ArtifactNotFound { coordinates: String },
    
    #[error("Invalid Maven coordinates: {coordinates}")]
    InvalidCoordinates { coordinates: String },
    
    #[error("Storage error: {message}")]
    StorageError { message: String },
    
    #[error("XML generation error: {message}")]
    XmlGenerationError { message: String },
    
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

/// Error de listado de artefactos
#[derive(Debug, thiserror::Error)]
pub enum MavenArtifactListerError {
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Storage error: {message}")]
    StorageError { message: String },
    
    #[error("Network error: {message}")]
    NetworkError { message: String },
    
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

/// Error de caché de metadata
#[derive(Debug, thiserror::Error)]
pub enum MavenMetadataCacheError {
    #[error("Cache error: {message}")]
    CacheError { message: String },
    
    #[error("Serialization error: {message}")]
    SerializationError { message: String },
    
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

// Conversiones de errores
impl From<MavenMetadataGeneratorError> for MavenMetadataError {
    fn from(error: MavenMetadataGeneratorError) -> Self {
        match error {
            MavenMetadataGeneratorError::RepositoryNotFound { repository_id } => {
                MavenMetadataError::RepositoryNotFound { repository_id }
            }
            MavenMetadataGeneratorError::ArtifactNotFound { coordinates } => {
                MavenMetadataError::ArtifactNotFound { coordinates }
            }
            MavenMetadataGeneratorError::InvalidCoordinates { coordinates } => {
                MavenMetadataError::InvalidCoordinates { coordinates }
            }
            MavenMetadataGeneratorError::StorageError { message } => {
                MavenMetadataError::StorageError { message }
            }
            MavenMetadataGeneratorError::XmlGenerationError { message } => {
                MavenMetadataError::XmlGenerationError { message }
            }
            MavenMetadataGeneratorError::InternalError { message } => {
                MavenMetadataError::InternalError { message }
            }
        }
    }
}

impl From<MavenArtifactListerError> for MavenMetadataError {
    fn from(error: MavenArtifactListerError) -> Self {
        match error {
            MavenArtifactListerError::RepositoryNotFound { repository_id } => {
                MavenMetadataError::RepositoryNotFound { repository_id }
            }
            MavenArtifactListerError::StorageError { message } => {
                MavenMetadataError::StorageError { message }
            }
            MavenArtifactListerError::NetworkError { message } => {
                MavenMetadataError::NetworkError { message }
            }
            MavenArtifactListerError::InternalError { message } => {
                MavenMetadataError::InternalError { message }
            }
        }
    }
}

impl From<MavenMetadataCacheError> for MavenMetadataError {
    fn from(error: MavenMetadataCacheError) -> Self {
        match error {
            MavenMetadataCacheError::CacheError { message } => {
                MavenMetadataError::CacheError { message }
            }
            MavenMetadataCacheError::SerializationError { message } => {
                MavenMetadataError::InternalError { message }
            }
            MavenMetadataCacheError::InternalError { message } => {
                MavenMetadataError::InternalError { message }
            }
        }
    }
}

// Implementaciones mock para testing
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    
    /// Mock para MavenMetadataGenerator
    pub struct MockMavenMetadataGenerator {
        pub metadata: Mutex<HashMap<String, MavenMetadataDto>>,
    }
    
    impl MockMavenMetadataGenerator {
        pub fn new() -> Self {
            Self {
                metadata: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_metadata(&self, key: String, metadata: MavenMetadataDto) {
            self.metadata.lock().unwrap().insert(key, metadata);
        }
    }
    
    #[async_trait]
    impl MavenMetadataGenerator for MockMavenMetadataGenerator {
        async fn generate_metadata(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<MavenMetadataDto, MavenMetadataGeneratorError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            self.metadata.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| MavenMetadataGeneratorError::ArtifactNotFound {
                    coordinates: coordinates.to_string(),
                })
        }
        
        async fn generate_metadata_xml(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<String, MavenMetadataGeneratorError> {
            let metadata = self.generate_metadata(coordinates, repository_id).await?;
            
            // Generar XML simple para testing
            Ok(format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>{}</groupId>
  <artifactId>{}</artifactId>
  <versioning>
    <latest>{}</latest>
    <release>{}</release>
    <versions>
{}
    </versions>
    <lastUpdated>{}</lastUpdated>
  </versioning>
</metadata>"#,
                metadata.group_id,
                metadata.artifact_id,
                metadata.versioning.latest,
                metadata.versioning.release,
                metadata.versioning.versions.iter()
                    .map(|v| format!("      <version>{}</version>", v))
                    .collect::<Vec<_>>()
                    .join("\n"),
                metadata.versioning.last_updated
            ))
        }
    }
    
    /// Mock para MavenArtifactLister
    pub struct MockMavenArtifactLister {
        pub artifacts: Mutex<HashMap<String, Vec<MavenVersion>>>,
    }
    
    impl MockMavenArtifactLister {
        pub fn new() -> Self {
            Self {
                artifacts: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_versions(&self, key: String, versions: Vec<MavenVersion>) {
            self.artifacts.lock().unwrap().insert(key, versions);
        }
    }
    
    #[async_trait]
    impl MavenArtifactLister for MockMavenArtifactLister {
        async fn list_versions(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<Vec<MavenVersion>, MavenArtifactListerError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            self.artifacts.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| MavenArtifactListerError::StorageError {
                    message: "Artifact not found".to_string(),
                })
        }
        
        async fn list_artifacts_in_group(
            &self,
            group_id: &str,
            repository_id: &str,
        ) -> Result<Vec<String>, MavenArtifactListerError> {
            let prefix = format!("{}:{}:", repository_id, group_id);
            let artifacts: Vec<String> = self.artifacts.lock().unwrap()
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .map(|key| key.strip_prefix(&prefix).unwrap().to_string())
                .collect();
            
            if artifacts.is_empty() {
                return Err(MavenArtifactListerError::StorageError {
                    message: "No artifacts found in group".to_string(),
                });
            }
            
            Ok(artifacts)
        }
        
        async fn artifact_exists(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<bool, MavenArtifactListerError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            Ok(self.artifacts.lock().unwrap().contains_key(&key))
        }
    }
    
    /// Mock para MavenMetadataCache
    pub struct MockMavenMetadataCache {
        pub cache: Mutex<HashMap<String, MavenMetadataDto>>,
    }
    
    impl MockMavenMetadataCache {
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_to_cache(&self, key: String, metadata: MavenMetadataDto) {
            self.cache.lock().unwrap().insert(key, metadata);
        }
    }
    
    #[async_trait]
    impl MavenMetadataCache for MockMavenMetadataCache {
        async fn get_cached_metadata(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<Option<MavenMetadataDto>, MavenMetadataCacheError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            Ok(self.cache.lock().unwrap().get(&key).cloned())
        }
        
        async fn cache_metadata(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
            metadata: &MavenMetadataDto,
        ) -> Result<(), MavenMetadataCacheError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            self.cache.lock().unwrap().insert(key, metadata.clone());
            Ok(())
        }
        
        async fn invalidate_cache(
            &self,
            coordinates: &MavenCoordinates,
            repository_id: &str,
        ) -> Result<(), MavenMetadataCacheError> {
            let key = format!("{}:{}:{}", repository_id, coordinates.group_id, coordinates.artifact_id);
            
            self.cache.lock().unwrap().remove(&key);
            Ok(())
        }
        
        async fn invalidate_repository_cache(
            &self,
            repository_id: &str,
        ) -> Result<(), MavenMetadataCacheError> {
            let prefix = format!("{}:", repository_id);
            
            self.cache.lock().unwrap().retain(|key, _| !key.starts_with(&prefix));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::maven::MavenCoordinates;
    
    #[tokio::test]
    async fn test_mock_maven_metadata_generator() {
        let generator = MockMavenMetadataGenerator::new();
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string(), "1.1.0".to_string()])
            .build();
        
        generator.add_metadata("test-repo:com.example:test-artifact".to_string(), metadata.clone());
        
        let result = generator.generate_metadata(&coordinates, "test-repo").await.unwrap();
        assert_eq!(result.group_id, "com.example");
        assert_eq!(result.artifact_id, "test-artifact");
    }
    
    #[tokio::test]
    async fn test_mock_maven_artifact_lister() {
        let lister = MockMavenArtifactLister::new();
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        
        let versions = vec![
            MavenVersion::new("1.0.0").unwrap(),
            MavenVersion::new("1.1.0").unwrap(),
        ];
        
        lister.add_versions("test-repo:com.example:test-artifact".to_string(), versions.clone());
        
        let result = lister.list_versions(&coordinates, "test-repo").await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].to_string(), "1.0.0");
    }
    
    #[tokio::test]
    async fn test_mock_maven_metadata_cache() {
        let cache = MockMavenMetadataCache::new();
        let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
        
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "test-artifact".to_string())
            .versions(vec!["1.0.0".to_string()])
            .build();
        
        // Cache metadata
        cache.cache_metadata(&coordinates, "test-repo", &metadata).await.unwrap();
        
        // Get cached metadata
        let cached = cache.get_cached_metadata(&coordinates, "test-repo").await.unwrap();
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().group_id, "com.example");
        
        // Invalidate cache
        cache.invalidate_cache(&coordinates, "test-repo").await.unwrap();
        
        // Check cache is empty
        let cached_after = cache.get_cached_metadata(&coordinates, "test-repo").await.unwrap();
        assert!(cached_after.is_none());
    }
}