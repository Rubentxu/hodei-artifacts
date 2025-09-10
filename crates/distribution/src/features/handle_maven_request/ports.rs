// crates/distribution/src/features/handle_maven_request/ports.rs

//! Puertos segregados específicos para el feature Handle Maven Request
//! 
//! Estas interfaces son EXCLUSIVAS de este feature y no son compartidas con otros features.
//! Cada feature define sus PROPIOS puertos, incluso si son similares a otros features.

use async_trait::async_trait;
use std::sync::Arc;
use crate::domain::maven::coordinates::MavenCoordinates;
use super::dto::{MavenGetArtifactRequest, MavenGetArtifactResponse, MavenPutArtifactRequest, MavenPutArtifactResponse};

/// Error de lectura específico de este feature
#[derive(Debug, thiserror::Error)]
pub enum MavenReadError {
    #[error("Artifact not found: {0}")]
    NotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
}

/// Error de escritura específico de este feature
#[derive(Debug, thiserror::Error)]
pub enum MavenWriteError {
    #[error("Write failed: {0}")]
    WriteFailed(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Artifact already exists: {0}")]
    AlreadyExists(String),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    #[error("Overwrite not allowed: {0}")]
    OverwriteNotAllowed(String),
}

/// Puerto para leer artefactos Maven - INTERFAZ SEGREGADA
#[async_trait]
pub trait MavenArtifactReader {
    /// Leer un artefacto Maven completo
    async fn read_artifact(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<Vec<u8>, MavenReadError>;
    
    /// Leer metadata de un artefacto (headers HTTP)
    async fn read_artifact_metadata(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<ArtifactMetadata, MavenReadError>;
    
    /// Verificar si un artefacto existe
    async fn artifact_exists(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<bool, MavenReadError>;
}

/// Puerto para escribir artefactos Maven - INTERFAZ SEGREGADA
#[async_trait]
pub trait MavenArtifactWriter {
    /// Escribir un artefacto Maven completo
    async fn write_artifact(&self, coordinates: &MavenCoordinates, content: &[u8], repository_id: &str, overwrite: bool) -> Result<(), MavenWriteError>;
    
    /// Escribir metadata de un artefacto
    async fn write_artifact_metadata(&self, coordinates: &MavenCoordinates, metadata: &ArtifactMetadata, repository_id: &str) -> Result<(), MavenWriteError>;
}

/// Puerto para gestionar repositorios Maven - INTERFAZ SEGREGADA
#[async_trait]
pub trait MavenRepositoryManager {
    /// Verificar si un repositorio existe
    async fn repository_exists(&self, repository_id: &str) -> Result<bool, MavenReadError>;
    
    /// Obtener información del repositorio
    async fn get_repository_info(&self, repository_id: &str) -> Result<RepositoryInfo, MavenReadError>;
}

/// Puerto para gestionar permisos - INTERFAZ SEGREGADA
#[async_trait]
pub trait MavenPermissionChecker {
    /// Verificar si el usuario tiene permiso para leer
    async fn can_read(&self, user_id: &str, repository_id: &str, coordinates: &MavenCoordinates) -> Result<bool, MavenReadError>;
    
    /// Verificar si el usuario tiene permiso para escribir
    async fn can_write(&self, user_id: &str, repository_id: &str, coordinates: &MavenCoordinates) -> Result<bool, MavenWriteError>;
}

/// Metadata de un artefacto para headers HTTP
#[derive(Debug, Clone)]
pub struct ArtifactMetadata {
    pub content_length: usize,
    pub content_type: String,
    pub last_modified: String,
    pub etag: String,
}

/// Información del repositorio
#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    pub id: String,
    pub name: String,
    pub repository_type: String,
    pub allow_snapshots: bool,
    pub allow_releases: bool,
}

/// Adaptadores para testing (solo compilan en tests)
#[cfg(test)]
pub mod test {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    
    /// Mock para MavenArtifactReader
    pub struct MockMavenArtifactReader {
        pub artifacts: Mutex<HashMap<String, Vec<u8>>>,
        pub metadata: Mutex<HashMap<String, ArtifactMetadata>>,
    }
    
    impl MockMavenArtifactReader {
        pub fn new() -> Self {
            Self {
                artifacts: Mutex::new(HashMap::new()),
                metadata: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_artifact(&self, coordinates: &MavenCoordinates, repository_id: &str, content: Vec<u8>) {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            self.artifacts.lock().unwrap().insert(key.clone(), content);
            
            // Agregar metadata por defecto
            let metadata = ArtifactMetadata {
                content_length: 1024,
                content_type: "application/java-archive".to_string(),
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                etag: "etag123".to_string(),
            };
            self.metadata.lock().unwrap().insert(key, metadata);
        }
    }
    
    #[async_trait]
    impl MavenArtifactReader for MockMavenArtifactReader {
        async fn read_artifact(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<Vec<u8>, MavenReadError> {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            self.artifacts.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| MavenReadError::NotFound(key))
        }
        
        async fn read_artifact_metadata(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<ArtifactMetadata, MavenReadError> {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            self.metadata.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| MavenReadError::NotFound(key))
        }
        
        async fn artifact_exists(&self, coordinates: &MavenCoordinates, repository_id: &str) -> Result<bool, MavenReadError> {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            Ok(self.artifacts.lock().unwrap().contains_key(&key))
        }
    }
    
    /// Mock para MavenArtifactWriter
    pub struct MockMavenArtifactWriter {
        pub artifacts: Mutex<HashMap<String, Vec<u8>>>,
        pub metadata: Mutex<HashMap<String, ArtifactMetadata>>,
        pub allow_overwrite: bool,
    }
    
    impl MockMavenArtifactWriter {
        pub fn new() -> Self {
            Self {
                artifacts: Mutex::new(HashMap::new()),
                metadata: Mutex::new(HashMap::new()),
                allow_overwrite: true,
            }
        }
        
        pub fn set_allow_overwrite(&mut self, allow: bool) {
            self.allow_overwrite = allow;
        }
    }
    
    #[async_trait]
    impl MavenArtifactWriter for MockMavenArtifactWriter {
        async fn write_artifact(&self, coordinates: &MavenCoordinates, content: &[u8], repository_id: &str, overwrite: bool) -> Result<(), MavenWriteError> {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            
            if !overwrite && !self.allow_overwrite {
                if self.artifacts.lock().unwrap().contains_key(&key) {
                    return Err(MavenWriteError::OverwriteNotAllowed(key));
                }
            }
            
            self.artifacts.lock().unwrap().insert(key.clone(), content.to_vec());
            
            // Actualizar metadata
            let metadata = ArtifactMetadata {
                content_length: content.len(),
                content_type: "application/java-archive".to_string(),
                last_modified: OffsetDateTime::now_utc().to_string(),
                etag: format!("etag{}", content.len()),
            };
            self.metadata.lock().unwrap().insert(key, metadata);
            
            Ok(())
        }
        
        async fn write_artifact_metadata(&self, coordinates: &MavenCoordinates, metadata: &ArtifactMetadata, repository_id: &str) -> Result<(), MavenWriteError> {
            let key = format!("{}:{}", repository_id, coordinates.to_path());
            self.metadata.lock().unwrap().insert(key, metadata.clone());
            Ok(())
        }
    }
    
    /// Mock para MavenRepositoryManager
    pub struct MockMavenRepositoryManager {
        pub repositories: Mutex<HashMap<String, RepositoryInfo>>,
    }
    
    impl MockMavenRepositoryManager {
        pub fn new() -> Self {
            let mut repositories = HashMap::new();
            repositories.insert("test-repo".to_string(), RepositoryInfo {
                id: "test-repo".to_string(),
                name: "Test Repository".to_string(),
                repository_type: "hosted".to_string(),
                allow_snapshots: true,
                allow_releases: true,
            });
            
            Self {
                repositories: Mutex::new(repositories),
            }
        }
        
        pub fn add_repository(&self, info: RepositoryInfo) {
            self.repositories.lock().unwrap().insert(info.id.clone(), info);
        }
    }
    
    #[async_trait]
    impl MavenRepositoryManager for MockMavenRepositoryManager {
        async fn repository_exists(&self, repository_id: &str) -> Result<bool, MavenReadError> {
            Ok(self.repositories.lock().unwrap().contains_key(repository_id))
        }
        
        async fn get_repository_info(&self, repository_id: &str) -> Result<RepositoryInfo, MavenReadError> {
            self.repositories.lock().unwrap()
                .get(repository_id)
                .cloned()
                .ok_or_else(|| MavenReadError::RepositoryNotFound(repository_id.to_string()))
        }
    }
    
    /// Mock para MavenPermissionChecker
    pub struct MockMavenPermissionChecker {
        pub allow_read: bool,
        pub allow_write: bool,
    }
    
    impl MockMavenPermissionChecker {
        pub fn new() -> Self {
            Self {
                allow_read: true,
                allow_write: true,
            }
        }
        
        pub fn set_allow_read(&mut self, allow: bool) {
            self.allow_read = allow;
        }
        
        pub fn set_allow_write(&mut self, allow: bool) {
            self.allow_write = allow;
        }
    }
    
    #[async_trait]
    impl MavenPermissionChecker for MockMavenPermissionChecker {
        async fn can_read(&self, _user_id: &str, _repository_id: &str, _coordinates: &MavenCoordinates) -> Result<bool, MavenReadError> {
            Ok(self.allow_read)
        }
        
        async fn can_write(&self, _user_id: &str, _repository_id: &str, _coordinates: &MavenCoordinates) -> Result<bool, MavenWriteError> {
            if self.allow_write {
                Ok(true)
            } else {
                Err(MavenWriteError::PermissionDenied("Write permission denied".to_string()))
            }
        }
    }
}