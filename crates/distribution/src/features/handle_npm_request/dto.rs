// crates/distribution/src/features/handle_npm_request/dto.rs

//! DTOs específicos para el feature Handle NPM Request

use crate::domain::npm::{NpmPackageName, NpmVersion};
use serde::{Serialize, Deserialize};

/// Request para obtener un paquete npm (.tgz)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetPackageRequest {
    pub package_name: NpmPackageName,
    pub version: NpmVersion,
    pub repository_id: String,
}

/// Response para obtener un paquete npm (.tgz)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetPackageResponse {
    pub content: Vec<u8>,
    pub content_type: String,
    pub content_length: usize,
    pub last_modified: Option<time::OffsetDateTime>,
    pub etag: Option<String>,
    pub package_name: String,
    pub version: String,
    pub integrity: Option<String>,
}

/// Request para obtener metadata de un paquete (package.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetPackageJsonRequest {
    pub package_name: NpmPackageName,
    pub version: Option<NpmVersion>, // None para obtener la última versión
    pub repository_id: String,
}

/// Response para obtener metadata de un paquete (package.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetPackageJsonResponse {
    pub package_json: serde_json::Value,
    pub content_type: String,
    pub content_length: usize,
    pub last_modified: Option<time::OffsetDateTime>,
    pub etag: Option<String>,
    pub package_name: String,
    pub version: String,
}

/// Request para publicar un paquete npm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPutPackageRequest {
    pub package_name: NpmPackageName,
    pub version: NpmVersion,
    pub content: Vec<u8>,
    pub content_type: String,
    pub repository_id: String,
    pub overwrite: bool,
    pub metadata: Option<serde_json::Value>, // package.json opcional
}

/// Response para publicar un paquete npm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPutPackageResponse {
    pub success: bool,
    pub message: String,
    pub package_name: String,
    pub version: String,
    pub tarball_url: String,
    pub size_bytes: usize,
    pub published_at: time::OffsetDateTime,
}

/// Request para verificar existencia de un paquete (HEAD)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmHeadPackageRequest {
    pub package_name: NpmPackageName,
    pub version: NpmVersion,
    pub repository_id: String,
}

/// Response para verificar existencia de un paquete (HEAD)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmHeadPackageResponse {
    pub exists: bool,
    pub content_length: Option<usize>,
    pub last_modified: Option<time::OffsetDateTime>,
    pub etag: Option<String>,
    pub package_name: String,
    pub version: String,
}

/// Request para obtener información del repositorio npm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetRepositoryInfoRequest {
    pub package_name: NpmPackageName,
    pub repository_id: String,
}

/// Response para obtener información del repositorio npm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetRepositoryInfoResponse {
    pub package_json: serde_json::Value, // Metadata completa del repositorio
    pub content_type: String,
    pub content_length: usize,
    pub last_modified: Option<time::OffsetDateTime>,
    pub etag: Option<String>,
    pub package_name: String,
    pub versions: Vec<String>,
    pub dist_tags: std::collections::HashMap<String, String>,
}

/// Request para buscar paquetes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmSearchRequest {
    pub query: String,
    pub repository_id: String,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Response para búsqueda de paquetes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmSearchResponse {
    pub packages: Vec<NpmSearchResult>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Resultado de búsqueda
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmSearchResult {
    pub package: NpmPackageName,
    pub description: Option<String>,
    pub version: String,
    pub keywords: Vec<String>,
    pub author: Option<String>,
    pub date: Option<time::OffsetDateTime>,
    pub links: std::collections::HashMap<String, String>,
}

/// Request para obtener dist-tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetDistTagsRequest {
    pub package_name: NpmPackageName,
    pub repository_id: String,
}

/// Response para dist-tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGetDistTagsResponse {
    pub dist_tags: std::collections::HashMap<String, String>,
    pub package_name: String,
}

/// Request para actualizar dist-tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmUpdateDistTagsRequest {
    pub package_name: NpmPackageName,
    pub tag: String,
    pub version: NpmVersion,
    pub repository_id: String,
}

/// Response para actualizar dist-tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmUpdateDistTagsResponse {
    pub success: bool,
    pub message: String,
    pub package_name: String,
    pub tag: String,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::{NpmPackageName, NpmVersion};
    
    #[test]
    fn test_npm_get_package_request() {
        let name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmGetPackageRequest {
            package_name: name,
            version,
            repository_id: "npm-repo".to_string(),
        };
        
        assert_eq!(request.package_name.full_name(), "test-package");
        assert_eq!(request.version.to_string(), "1.0.0");
        assert_eq!(request.repository_id, "npm-repo");
    }
    
    #[test]
    fn test_npm_get_package_response() {
        let response = NpmGetPackageResponse {
            content: b"test content".to_vec(),
            content_type: "application/octet-stream".to_string(),
            content_length: 12,
            last_modified: None,
            etag: Some("etag123".to_string()),
            package_name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            integrity: Some("sha512-abc123".to_string()),
        };
        
        assert_eq!(response.content, b"test content");
        assert_eq!(response.content_length, 12);
        assert_eq!(response.etag, Some("etag123".to_string()));
    }
    
    #[test]
    fn test_npm_put_package_request() {
        let name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmPutPackageRequest {
            package_name: name,
            version,
            content: b"package content".to_vec(),
            content_type: "application/octet-stream".to_string(),
            repository_id: "npm-repo".to_string(),
            overwrite: false,
            metadata: None,
        };
        
        assert_eq!(request.content, b"package content");
        assert!(!request.overwrite);
    }
    
    #[test]
    fn test_npm_search_result() {
        let name = NpmPackageName::new("test-package").unwrap();
        
        let result = NpmSearchResult {
            package: name,
            description: Some("A test package".to_string()),
            version: "1.0.0".to_string(),
            keywords: vec!["test".to_string(), "npm".to_string()],
            author: Some("Test Author".to_string()),
            date: None,
            links: std::collections::HashMap::new(),
        };
        
        assert_eq!(result.package.full_name(), "test-package");
        assert_eq!(result.description, Some("A test package".to_string()));
        assert_eq!(result.keywords.len(), 2);
    }
}