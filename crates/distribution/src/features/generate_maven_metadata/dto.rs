// crates/distribution/src/features/generate_maven_metadata/dto.rs

//! Data Transfer Objects para la generación de Maven metadata

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use crate::domain::maven::{MavenCoordinates, MavenVersion, MavenSnapshotVersion};

/// Request para generar Maven metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateMavenMetadataRequest {
    /// Coordenadas Maven del grupo y artefacto
    pub coordinates: MavenCoordinates,
    /// ID del repositorio
    pub repository_id: String,
    /// Forzar regeneración (ignorar caché)
    pub force_regenerate: bool,
}

/// Response con el metadata generado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateMavenMetadataResponse {
    /// Metadata generado
    pub metadata: MavenMetadataDto,
    /// Indica si fue generado o recuperado de caché
    pub from_cache: bool,
    /// Timestamp de generación
    pub generated_at: DateTime<Utc>,
}

/// DTO para Maven metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenMetadataDto {
    /// Coordenadas del artefacto
    pub group_id: String,
    pub artifact_id: String,
    /// Versión actual (si es un artefacto específico)
    pub version: Option<String>,
    /// Información de versioning
    pub versioning: MavenMetadataVersioningDto,
}

/// DTO para información de versioning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenMetadataVersioningDto {
    /// Última versión disponible
    pub latest: String,
    /// Última versión de lanzamiento (no snapshot)
    pub release: String,
    /// Lista de todas las versiones
    pub versions: Vec<String>,
    /// Información de snapshot (si aplica)
    pub snapshot: Option<MavenMetadataSnapshotDto>,
    /// Timestamp de última actualización
    pub last_updated: String,
}

/// DTO para información de snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenMetadataSnapshotDto {
    /// Timestamp del snapshot
    pub timestamp: String,
    /// Número de build
    pub build_number: i32,
    /// Es local
    pub local_copy: bool,
}

/// DTO para versión de snapshot específica
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenMetadataSnapshotVersionDto {
    /// Clasificador (ej: sources, javadoc)
    pub classifier: Option<String>,
    /// Extensión del archivo
    pub extension: String,
    /// Versión del snapshot
    pub value: String,
    /// Timestamp de actualización
    pub updated: String,
}

/// Error de generación de Maven metadata
#[derive(Debug, thiserror::Error)]
pub enum MavenMetadataError {
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Artifact not found: {coordinates}")]
    ArtifactNotFound { coordinates: String },
    
    #[error("Invalid Maven coordinates: {coordinates}")]
    InvalidCoordinates { coordinates: String },
    
    #[error("Storage error: {message}")]
    StorageError { message: String },
    
    #[error("Cache error: {message}")]
    CacheError { message: String },
    
    #[error("XML generation error: {message}")]
    XmlGenerationError { message: String },
    
    #[error("Permission denied: {message}")]
    PermissionDenied { message: String },
    
    #[error("Network error: {message}")]
    NetworkError { message: String },
    
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

/// Response de error para la API
#[derive(Debug, Serialize, Deserialize)]
pub struct MavenMetadataErrorResponse {
    pub error: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub request_id: Option<String>,
}

impl MavenMetadataError {
    /// Convertir a HTTP status code
    pub fn to_http_status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        
        match self {
            MavenMetadataError::RepositoryNotFound { .. } => StatusCode::NOT_FOUND,
            MavenMetadataError::ArtifactNotFound { .. } => StatusCode::NOT_FOUND,
            MavenMetadataError::InvalidCoordinates { .. } => StatusCode::BAD_REQUEST,
            MavenMetadataError::StorageError { .. } => StatusCode::SERVICE_UNAVAILABLE,
            MavenMetadataError::CacheError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            MavenMetadataError::XmlGenerationError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            MavenMetadataError::PermissionDenied { .. } => StatusCode::FORBIDDEN,
            MavenMetadataError::NetworkError { .. } => StatusCode::SERVICE_UNAVAILABLE,
            MavenMetadataError::InternalError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
    
    /// Crear response de error
    pub fn to_error_response(&self, request_id: Option<String>) -> MavenMetadataErrorResponse {
        MavenMetadataErrorResponse {
            error: format!("{:?}", self),
            message: self.to_string(),
            timestamp: Utc::now(),
            request_id,
        }
    }
}

/// Builder para MavenMetadataDto
pub struct MavenMetadataDtoBuilder {
    group_id: String,
    artifact_id: String,
    version: Option<String>,
    versions: Vec<String>,
    snapshot_info: Option<MavenMetadataSnapshotDto>,
}

impl MavenMetadataDtoBuilder {
    pub fn new(group_id: String, artifact_id: String) -> Self {
        Self {
            group_id,
            artifact_id,
            version: None,
            versions: Vec::new(),
            snapshot_info: None,
        }
    }
    
    pub fn version(mut self, version: String) -> Self {
        self.version = Some(version);
        self
    }
    
    pub fn versions(mut self, versions: Vec<String>) -> Self {
        self.versions = versions;
        self
    }
    
    pub fn snapshot_info(mut self, snapshot: MavenMetadataSnapshotDto) -> Self {
        self.snapshot_info = Some(snapshot);
        self
    }
    
    pub fn build(self) -> MavenMetadataDto {
        let latest = self.versions.last()
            .cloned()
            .unwrap_or_else(|| "0.0.0".to_string());
        
        let release = self.versions.iter()
            .filter(|v| !v.contains("-SNAPSHOT"))
            .last()
            .cloned()
            .unwrap_or_else(|| latest.clone());
        
        let last_updated = Utc::now().format("%Y%m%d%H%M%S").to_string();
        
        MavenMetadataDto {
            group_id: self.group_id,
            artifact_id: self.artifact_id,
            version: self.version,
            versioning: MavenMetadataVersioningDto {
                latest,
                release,
                versions: self.versions,
                snapshot: self.snapshot_info,
                last_updated,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_maven_metadata_dto_builder() {
        let metadata = MavenMetadataDtoBuilder::new("com.example".to_string(), "my-artifact".to_string())
            .versions(vec![
                "1.0.0".to_string(),
                "1.1.0".to_string(),
                "2.0.0-SNAPSHOT".to_string(),
            ])
            .build();
        
        assert_eq!(metadata.group_id, "com.example");
        assert_eq!(metadata.artifact_id, "my-artifact");
        assert_eq!(metadata.versioning.latest, "2.0.0-SNAPSHOT");
        assert_eq!(metadata.versioning.release, "1.1.0");
        assert_eq!(metadata.versioning.versions.len(), 3);
    }
    
    #[test]
    fn test_error_to_http_status() {
        let error = MavenMetadataError::RepositoryNotFound {
            repository_id: "test-repo".to_string(),
        };
        
        assert_eq!(error.to_http_status(), axum::http::StatusCode::NOT_FOUND);
    }
    
    #[test]
    fn test_error_to_error_response() {
        let error = MavenMetadataError::ArtifactNotFound {
            coordinates: "com.example:artifact:1.0.0".to_string(),
        };
        
        let response = error.to_error_response(Some("req-123".to_string()));
        
        assert!(response.message.contains("com.example:artifact:1.0.0"));
        assert_eq!(response.request_id, Some("req-123".to_string()));
    }
}