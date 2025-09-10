// crates/distribution/src/features/handle_maven_request/use_case.rs

//! Use cases específicos para el feature Handle Maven Request
//! 
//! Cada use case contiene la lógica de negocio pura para una operación específica.

use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, instrument, error};
use crate::domain::maven::validation::validate_maven_coordinates;
use super::dto::{
    MavenGetArtifactRequest, MavenGetArtifactResponse, 
    MavenPutArtifactRequest, MavenPutArtifactResponse,
    MavenHeadArtifactRequest, MavenHeadArtifactResponse,
    MavenListArtifactsRequest, MavenListArtifactsResponse
};
use super::ports::{
    MavenArtifactReader, MavenArtifactWriter, MavenRepositoryManager, 
    MavenPermissionChecker, MavenReadError, MavenWriteError
};

/// Use case para obtener un artefacto Maven
pub struct HandleMavenGetArtifactUseCase {
    artifact_reader: Arc<dyn MavenArtifactReader>,
    repository_manager: Arc<dyn MavenRepositoryManager>,
    permission_checker: Arc<dyn MavenPermissionChecker>,
}

impl HandleMavenGetArtifactUseCase {
    pub fn new(
        artifact_reader: Arc<dyn MavenArtifactReader>,
        repository_manager: Arc<dyn MavenRepositoryManager>,
        permission_checker: Arc<dyn MavenPermissionChecker>,
    ) -> Self {
        Self {
            artifact_reader,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(skip(self), fields(
        group_id = %request.coordinates.group_id,
        artifact_id = %request.coordinates.artifact_id,
        version = %request.coordinates.version,
        repository_id = %request.repository_id
    ))]
    pub async fn execute(&self, request: MavenGetArtifactRequest) -> Result<MavenGetArtifactResponse, MavenGetError> {
        info!("Processing Maven GET artifact request");
        
        // 1. Validar coordenadas (lógica de negocio pura)
        validate_maven_coordinates(&request.coordinates)
            .map_err(|e| MavenGetError::InvalidCoordinates(e))?;
        
        // 2. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            error!("Repository not found: {}", request.repository_id);
            return Err(MavenGetError::RepositoryNotFound(request.repository_id));
        }
        
        // 3. Verificar permisos de lectura
        if !self.permission_checker.can_read("system", &request.repository_id, &request.coordinates).await? {
            error!("Permission denied for reading artifact");
            return Err(MavenGetError::PermissionDenied);
        }
        
        // 4. Verificar que el artefacto existe
        if !self.artifact_reader.artifact_exists(&request.coordinates, &request.repository_id).await? {
            error!("Artifact not found: {}", request.coordinates);
            return Err(MavenGetError::ArtifactNotFound(request.coordinates.to_string()));
        }
        
        // 5. Leer el artefacto
        let content = self.artifact_reader.read_artifact(&request.coordinates, &request.repository_id).await
            .map_err(|e| MavenGetError::ReadFailed(e))?;
        
        // 6. Leer metadata para headers HTTP
        let metadata = self.artifact_reader.read_artifact_metadata(&request.coordinates, &request.repository_id).await
            .map_err(|e| MavenGetError::ReadFailed(e))?;
        
        info!("Successfully retrieved artifact: {} bytes", content.len());
        
        Ok(MavenGetArtifactResponse {
            content,
            content_type: metadata.content_type,
            content_length: metadata.content_length,
            last_modified: Some(metadata.last_modified),
            etag: Some(metadata.etag),
        })
    }
}

/// Use case para subir un artefacto Maven
pub struct HandleMavenPutArtifactUseCase {
    artifact_writer: Arc<dyn MavenArtifactWriter>,
    repository_manager: Arc<dyn MavenRepositoryManager>,
    permission_checker: Arc<dyn MavenPermissionChecker>,
}

impl HandleMavenPutArtifactUseCase {
    pub fn new(
        artifact_writer: Arc<dyn MavenArtifactWriter>,
        repository_manager: Arc<dyn MavenRepositoryManager>,
        permission_checker: Arc<dyn MavenPermissionChecker>,
    ) -> Self {
        Self {
            artifact_writer,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(skip(self), fields(
        group_id = %request.coordinates.group_id,
        artifact_id = %request.coordinates.artifact_id,
        version = %request.coordinates.version,
        repository_id = %request.repository_id,
        content_length = request.content.len()
    ))]
    pub async fn execute(&self, request: MavenPutArtifactRequest) -> Result<MavenPutArtifactResponse, MavenPutError> {
        info!("Processing Maven PUT artifact request");
        
        // 1. Validar coordenadas (lógica de negocio pura)
        validate_maven_coordinates(&request.coordinates)
            .map_err(|e| MavenPutError::InvalidCoordinates(e))?;
        
        // 2. Validar content type
        self.validate_content_type(&request.content_type)?;
        
        // 3. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            error!("Repository not found: {}", request.repository_id);
            return Err(MavenPutError::RepositoryNotFound(request.repository_id));
        }
        
        // 4. Verificar permisos de escritura
        if !self.permission_checker.can_write("system", &request.repository_id, &request.coordinates).await? {
            error!("Permission denied for writing artifact");
            return Err(MavenPutError::PermissionDenied);
        }
        
        // 5. Verificar políticas del repositorio (ej: snapshots permitidos)
        let repo_info = self.repository_manager.get_repository_info(&request.repository_id).await?;
        if request.coordinates.is_snapshot() && !repo_info.allow_snapshots {
            error!("Snapshots not allowed in repository: {}", request.repository_id);
            return Err(MavenPutError::SnapshotsNotAllowed);
        }
        
        if request.coordinates.is_release() && !repo_info.allow_releases {
            error!("Releases not allowed in repository: {}", request.repository_id);
            return Err(MavenPutError::ReleasesNotAllowed);
        }
        
        // 6. Escribir el artefacto
        self.artifact_writer.write_artifact(
            &request.coordinates,
            &request.content,
            &request.repository_id,
            request.overwrite
        ).await
        .map_err(|e| MavenPutError::WriteFailed(e))?;
        
        info!("Successfully uploaded artifact: {} bytes", request.content.len());
        
        Ok(MavenPutArtifactResponse {
            success: true,
            message: format!("Artifact {} uploaded successfully", request.coordinates),
            artifact_path: request.coordinates.to_path(),
            size_bytes: request.content.len(),
        })
    }
    
    fn validate_content_type(&self, content_type: &str) -> Result<(), MavenPutError> {
        let valid_types = [
            "application/java-archive",
            "application/xml",
            "text/xml",
            "application/octet-stream",
        ];
        
        if !valid_types.contains(&content_type) {
            error!("Invalid content type: {}", content_type);
            return Err(MavenPutError::InvalidContentType(content_type.to_string()));
        }
        
        Ok(())
    }
}

/// Use case para verificar la existencia de un artefacto (HEAD request)
pub struct HandleMavenHeadArtifactUseCase {
    artifact_reader: Arc<dyn MavenArtifactReader>,
    repository_manager: Arc<dyn MavenRepositoryManager>,
    permission_checker: Arc<dyn MavenPermissionChecker>,
}

impl HandleMavenHeadArtifactUseCase {
    pub fn new(
        artifact_reader: Arc<dyn MavenArtifactReader>,
        repository_manager: Arc<dyn MavenRepositoryManager>,
        permission_checker: Arc<dyn MavenPermissionChecker>,
    ) -> Self {
        Self {
            artifact_reader,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(skip(self), fields(
        group_id = %request.coordinates.group_id,
        artifact_id = %request.coordinates.artifact_id,
        version = %request.coordinates.version,
        repository_id = %request.repository_id
    ))]
    pub async fn execute(&self, request: MavenHeadArtifactRequest) -> Result<MavenHeadArtifactResponse, MavenHeadError> {
        info!("Processing Maven HEAD artifact request");
        
        // 1. Validar coordenadas
        validate_maven_coordinates(&request.coordinates)
            .map_err(|e| MavenHeadError::InvalidCoordinates(e))?;
        
        // 2. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            return Err(MavenHeadError::RepositoryNotFound(request.repository_id));
        }
        
        // 3. Verificar permisos de lectura
        if !self.permission_checker.can_read("system", &request.repository_id, &request.coordinates).await? {
            return Err(MavenHeadError::PermissionDenied);
        }
        
        // 4. Verificar existencia del artefacto
        let exists = self.artifact_reader.artifact_exists(&request.coordinates, &request.repository_id).await?;
        
        if exists {
            // Obtener metadata para headers
            let metadata = self.artifact_reader.read_artifact_metadata(&request.coordinates, &request.repository_id).await
                .map_err(|e| MavenHeadError::ReadFailed(e))?;
            
            Ok(MavenHeadArtifactResponse {
                exists: true,
                content_length: Some(metadata.content_length),
                last_modified: Some(metadata.last_modified),
                etag: Some(metadata.etag),
            })
        } else {
            Ok(MavenHeadArtifactResponse {
                exists: false,
                content_length: None,
                last_modified: None,
                etag: None,
            })
        }
    }
}

/// Errores específicos del feature Handle Maven Request
#[derive(Debug, thiserror::Error)]
pub enum MavenGetError {
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(crate::domain::maven::coordinates::MavenValidationError),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Read failed: {0}")]
    ReadFailed(MavenReadError),
}

#[derive(Debug, thiserror::Error)]
pub enum MavenPutError {
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(crate::domain::maven::coordinates::MavenValidationError),
    #[error("Invalid content type: {0}")]
    InvalidContentType(String),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Snapshots not allowed in repository")]
    SnapshotsNotAllowed,
    #[error("Releases not allowed in repository")]
    ReleasesNotAllowed,
    #[error("Write failed: {0}")]
    WriteFailed(MavenWriteError),
}

#[derive(Debug, thiserror::Error)]
pub enum MavenHeadError {
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(crate::domain::maven::coordinates::MavenValidationError),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Read failed: {0}")]
    ReadFailed(MavenReadError),
}

// Conversiones de errores
impl From<MavenReadError> for MavenGetError {
    fn from(error: MavenReadError) -> Self {
        MavenGetError::ReadFailed(error)
    }
}

impl From<MavenReadError> for MavenHeadError {
    fn from(error: MavenReadError) -> Self {
        MavenHeadError::ReadFailed(error)
    }
}

impl From<MavenWriteError> for MavenPutError {
    fn from(error: MavenWriteError) -> Self {
        MavenPutError::WriteFailed(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::maven::coordinates::MavenCoordinates;
    
    #[tokio::test]
    async fn test_get_artifact_use_case_success() {
        // Arrange
        let artifact_reader = Arc::new(ports::test::MockMavenArtifactReader::new());
        let repository_manager = Arc::new(ports::test::MockMavenRepositoryManager::new());
        let permission_checker = Arc::new(ports::test::MockMavenPermissionChecker::new());
        
        let coordinates = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        let content = b"test jar content".to_vec();
        artifact_reader.add_artifact(&coordinates, "test-repo", content.clone());
        
        let use_case = HandleMavenGetArtifactUseCase::new(
            artifact_reader,
            repository_manager,
            permission_checker,
        );
        
        let request = MavenGetArtifactRequest {
            coordinates,
            repository_id: "test-repo".to_string(),
        };
        
        // Act
        let result = use_case.execute(request).await;
        
        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, content);
        assert_eq!(response.content_length, content.len());
    }
    
    #[tokio::test]
    async fn test_put_artifact_use_case_success() {
        // Arrange
        let artifact_writer = Arc::new(ports::test::MockMavenArtifactWriter::new());
        let repository_manager = Arc::new(ports::test::MockMavenRepositoryManager::new());
        let permission_checker = Arc::new(ports::test::MockMavenPermissionChecker::new());
        
        let use_case = HandleMavenPutArtifactUseCase::new(
            artifact_writer,
            repository_manager,
            permission_checker,
        );
        
        let coordinates = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        let content = b"test jar content".to_vec();
        
        let request = MavenPutArtifactRequest {
            coordinates: coordinates.clone(),
            content: content.clone(),
            content_type: "application/java-archive".to_string(),
            repository_id: "test-repo".to_string(),
            overwrite: false,
        };
        
        // Act
        let result = use_case.execute(request).await;
        
        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.success);
        assert_eq!(response.size_bytes, content.len());
    }
}