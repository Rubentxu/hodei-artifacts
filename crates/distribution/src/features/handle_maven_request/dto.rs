// crates/distribution/src/features/handle_maven_request/dto.rs

//! DTOs específicos para el feature Handle Maven Request
//! 
//! Estos DTOs son específicos de este feature y no son compartidos con otros features.

use serde::{Serialize, Deserialize};
use crate::domain::maven::coordinates::MavenCoordinates;

/// Request para obtener un artefacto Maven
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenGetArtifactRequest {
    pub coordinates: MavenCoordinates,
    pub repository_id: String,
}

/// Response para obtener un artefacto Maven
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenGetArtifactResponse {
    pub content: Vec<u8>,
    pub content_type: String,
    pub content_length: usize,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
}

/// Request para subir un artefacto Maven
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenPutArtifactRequest {
    pub coordinates: MavenCoordinates,
    pub content: Vec<u8>,
    pub content_type: String,
    pub repository_id: String,
    pub overwrite: bool,
}

/// Response para subir un artefacto Maven
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenPutArtifactResponse {
    pub success: bool,
    pub message: String,
    pub artifact_path: String,
    pub size_bytes: usize,
}

/// Request para verificar la existencia de un artefacto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenHeadArtifactRequest {
    pub coordinates: MavenCoordinates,
    pub repository_id: String,
}

/// Response para verificar la existencia de un artefacto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenHeadArtifactResponse {
    pub exists: bool,
    pub content_length: Option<usize>,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
}

/// Request para listar artefactos en un grupo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenListArtifactsRequest {
    pub group_id: String,
    pub repository_id: String,
    pub recursive: bool,
}

/// Información de un artefacto en la lista
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenArtifactInfo {
    pub coordinates: MavenCoordinates,
    pub size_bytes: usize,
    pub last_modified: String,
}

/// Response para listar artefactos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenListArtifactsResponse {
    pub artifacts: Vec<MavenArtifactInfo>,
    pub total_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_maven_get_artifact_request() {
        let coordinates = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        let request = MavenGetArtifactRequest {
            coordinates: coordinates.clone(),
            repository_id: "maven-central".to_string(),
        };
        
        assert_eq!(request.coordinates.group_id, "com.example");
        assert_eq!(request.repository_id, "maven-central");
    }
    
    #[test]
    fn test_maven_put_artifact_response() {
        let response = MavenPutArtifactResponse {
            success: true,
            message: "Artifact uploaded successfully".to_string(),
            artifact_path: "com/example/my-app/1.0.0/my-app-1.0.0.jar".to_string(),
            size_bytes: 1024,
        };
        
        assert!(response.success);
        assert_eq!(response.size_bytes, 1024);
    }
}