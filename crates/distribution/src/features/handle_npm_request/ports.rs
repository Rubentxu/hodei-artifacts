// crates/distribution/src/features/handle_npm_request/ports.rs

//! Puertos segregados específicos para el feature Handle NPM Request
//! 
//! Cada feature define sus PROPIOS puertos, incluso si son similares a otros features.
//! Esto garantiza la independencia total y permite cambiar implementaciones sin
//! afectar otros features.

use async_trait::async_trait;
use std::sync::Arc;
use crate::domain::npm::{NpmPackageName, NpmVersion, NpmPackageMetadata, NpmRepositoryMetadata};
use super::dto::{
    NpmGetPackageRequest, NpmGetPackageResponse,
    NpmPutPackageRequest, NpmPutPackageResponse,
    NpmHeadPackageRequest, NpmHeadPackageResponse,
    NpmGetPackageJsonRequest, NpmGetPackageJsonResponse,
    NpmGetRepositoryInfoRequest, NpmGetRepositoryInfoResponse,
    NpmSearchRequest, NpmSearchResponse,
    NpmGetDistTagsRequest, NpmGetDistTagsResponse,
    NpmUpdateDistTagsRequest, NpmUpdateDistTagsResponse,
};

/// Error de lectura para operaciones npm
#[derive(Debug, thiserror::Error)]
pub enum NpmReadError {
    #[error("Package not found: {package_name}@{version}")]
    PackageNotFound { package_name: String, version: String },
    
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Permission denied for package: {package_name}")]
    PermissionDenied { package_name: String },
    
    #[error("Invalid package name: {0}")]
    InvalidPackageName(String),
    
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// Error de escritura para operaciones npm
#[derive(Debug, thiserror::Error)]
pub enum NpmWriteError {
    #[error("Package already exists: {package_name}@{version}")]
    PackageAlreadyExists { package_name: String, version: String },
    
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Permission denied for package: {package_name}")]
    PermissionDenied { package_name: String },
    
    #[error("Invalid package name: {0}")]
    InvalidPackageName(String),
    
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
    
    #[error("Invalid package content: {0}")]
    InvalidPackageContent(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Private package cannot be published: {package_name}")]
    PrivatePackage { package_name: String },
}

/// Puerto para leer paquetes npm (.tgz)
#[async_trait]
pub trait NpmPackageReader: Send + Sync {
    /// Leer un paquete npm (.tgz)
    async fn read_package(&self, request: &NpmGetPackageRequest) -> Result<NpmGetPackageResponse, NpmReadError>;
    
    /// Verificar si un paquete existe
    async fn package_exists(&self, request: &NpmHeadPackageRequest) -> Result<bool, NpmReadError>;
    
    /// Obtener metadata de un paquete (package.json)
    async fn read_package_json(&self, request: &NpmGetPackageJsonRequest) -> Result<NpmGetPackageJsonResponse, NpmReadError>;
    
    /// Obtener información del repositorio npm
    async fn read_repository_info(&self, request: &NpmGetRepositoryInfoRequest) -> Result<NpmGetRepositoryInfoResponse, NpmReadError>;
    
    /// Buscar paquetes
    async fn search_packages(&self, request: &NpmSearchRequest) -> Result<NpmSearchResponse, NpmReadError>;
    
    /// Obtener dist-tags
    async fn get_dist_tags(&self, request: &NpmGetDistTagsRequest) -> Result<NpmGetDistTagsResponse, NpmReadError>;
}

/// Puerto para escribir paquetes npm (.tgz)
#[async_trait]
pub trait NpmPackageWriter: Send + Sync {
    /// Escribir un paquete npm (.tgz)
    async fn write_package(&self, request: &NpmPutPackageRequest) -> Result<NpmPutPackageResponse, NpmWriteError>;
    
    /// Actualizar dist-tags
    async fn update_dist_tags(&self, request: &NpmUpdateDistTagsRequest) -> Result<NpmUpdateDistTagsResponse, NpmWriteError>;
}

/// Puerto para gestión de repositorios npm
#[async_trait]
pub trait NpmRepositoryManager: Send + Sync {
    /// Verificar si un repositorio existe
    async fn repository_exists(&self, repository_id: &str) -> Result<bool, NpmReadError>;
    
    /// Obtener información del repositorio
    async fn get_repository_info(&self, repository_id: &str) -> Result<NpmRepositoryInfo, NpmReadError>;
    
    /// Verificar si se permite publicar en el repositorio
    async fn can_publish(&self, repository_id: &str) -> Result<bool, NpmWriteError>;
    
    /// Obtener la URL base del repositorio
    async fn get_repository_base_url(&self, repository_id: &str) -> Result<String, NpmReadError>;
}

/// Puerto para control de permisos npm
#[async_trait]
pub trait NpmPermissionChecker: Send + Sync {
    /// Verificar si se puede leer un paquete
    async fn can_read_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmReadError>;
    
    /// Verificar si se puede escribir un paquete
    async fn can_write_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError>;
    
    /// Verificar si se puede actualizar dist-tags
    async fn can_update_dist_tags(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError>;
}

/// Información del repositorio npm
#[derive(Debug, Clone)]
pub struct NpmRepositoryInfo {
    pub repository_id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub allow_publish: bool,
    pub registry_url: String,
    pub max_package_size: Option<u64>,
    pub supported_formats: Vec<String>,
}

/// Adaptadores para testing (solo compilan en tests)
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    
    /// Mock para NpmPackageReader
    pub struct MockNpmPackageReader {
        pub packages: Mutex<HashMap<String, Vec<u8>>>,
        pub package_jsons: Mutex<HashMap<String, serde_json::Value>>,
        pub repository_info: Mutex<HashMap<String, NpmRepositoryInfo>>,
        pub search_results: Mutex<Vec<NpmSearchResult>>,
    }
    
    impl MockNpmPackageReader {
        pub fn new() -> Self {
            Self {
                packages: Mutex::new(HashMap::new()),
                package_jsons: Mutex::new(HashMap::new()),
                repository_info: Mutex::new(HashMap::new()),
                search_results: Mutex::new(Vec::new()),
            }
        }
        
        pub fn add_package(&self, key: String, content: Vec<u8>) {
            self.packages.lock().unwrap().insert(key, content);
        }
        
        pub fn add_package_json(&self, key: String, json: serde_json::Value) {
            self.package_jsons.lock().unwrap().insert(key, json);
        }
        
        pub fn add_repository_info(&self, repository_id: String, info: NpmRepositoryInfo) {
            self.repository_info.lock().unwrap().insert(repository_id, info);
        }
        
        pub fn add_search_result(&self, result: NpmSearchResult) {
            self.search_results.lock().unwrap().push(result);
        }
    }
    
    #[async_trait]
    impl NpmPackageReader for MockNpmPackageReader {
        async fn read_package(&self, request: &NpmGetPackageRequest) -> Result<NpmGetPackageResponse, NpmReadError> {
            let key = format!("{}@{}", request.package_name.full_name(), request.version);
            
            if let Some(content) = self.packages.lock().unwrap().get(&key) {
                Ok(NpmGetPackageResponse {
                    content: content.clone(),
                    content_type: "application/octet-stream".to_string(),
                    content_length: content.len(),
                    last_modified: None,
                    etag: Some(format!("\"{}\"", key)),
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                    integrity: None,
                })
            } else {
                Err(NpmReadError::PackageNotFound {
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                })
            }
        }
        
        async fn package_exists(&self, request: &NpmHeadPackageRequest) -> Result<bool, NpmReadError> {
            let key = format!("{}@{}", request.package_name.full_name(), request.version);
            Ok(self.packages.lock().unwrap().contains_key(&key))
        }
        
        async fn read_package_json(&self, request: &NpmGetPackageJsonRequest) -> Result<NpmGetPackageJsonResponse, NpmReadError> {
            let key = if let Some(ref version) = request.version {
                format!("{}@{}", request.package_name.full_name(), version)
            } else {
                // Buscar la última versión
                let latest_key = self.package_jsons.lock().unwrap()
                    .keys()
                    .filter(|k| k.starts_with(&format!("{}@", request.package_name.full_name())))
                    .max()
                    .cloned();
                
                latest_key.ok_or_else(|| NpmReadError::PackageNotFound {
                    package_name: request.package_name.full_name().to_string(),
                    version: "latest".to_string(),
                })?
            };
            
            if let Some(json) = self.package_jsons.lock().unwrap().get(&key) {
                Ok(NpmGetPackageJsonResponse {
                    package_json: json.clone(),
                    content_type: "application/json".to_string(),
                    content_length: serde_json::to_string(json).unwrap().len(),
                    last_modified: None,
                    etag: Some(format!("\"{}\"", key)),
                    package_name: request.package_name.full_name().to_string(),
                    version: key.split('@').last().unwrap().to_string(),
                })
            } else {
                Err(NpmReadError::PackageNotFound {
                    package_name: request.package_name.full_name().to_string(),
                    version: key.split('@').last().unwrap().to_string(),
                })
            }
        }
        
        async fn read_repository_info(&self, request: &NpmGetRepositoryInfoRequest) -> Result<NpmGetRepositoryInfoResponse, NpmReadError> {
            if let Some(info) = self.repository_info.lock().unwrap().get(&request.repository_id) {
                let mut versions = Vec::new();
                let mut dist_tags = std::collections::HashMap::new();
                
                // Construir información del repositorio
                for key in self.package_jsons.lock().unwrap().keys() {
                    if key.starts_with(&format!("{}@", request.package_name.full_name())) {
                        let version = key.split('@').last().unwrap().to_string();
                        versions.push(version);
                    }
                }
                
                versions.sort();
                
                // Agregar dist-tags básicos
                if !versions.is_empty() {
                    dist_tags.insert("latest".to_string(), versions.last().unwrap().clone());
                }
                
                Ok(NpmGetRepositoryInfoResponse {
                    package_json: serde_json::json!({
                        "name": request.package_name.full_name(),
                        "versions": versions,
                        "dist-tags": dist_tags
                    }),
                    content_type: "application/json".to_string(),
                    content_length: 0,
                    last_modified: None,
                    etag: None,
                    package_name: request.package_name.full_name().to_string(),
                    versions,
                    dist_tags,
                })
            } else {
                Err(NpmReadError::RepositoryNotFound {
                    repository_id: request.repository_id.clone(),
                })
            }
        }
        
        async fn search_packages(&self, request: &NpmSearchRequest) -> Result<NpmSearchResponse, NpmReadError> {
            let results = self.search_results.lock().unwrap()
                .iter()
                .filter(|result| {
                    result.package.full_name().to_lowercase().contains(&request.query.to_lowercase()) ||
                    result.description.as_ref().map(|d| d.to_lowercase().contains(&request.query.to_lowercase())).unwrap_or(false) ||
                    result.keywords.iter().any(|k| k.to_lowercase().contains(&request.query.to_lowercase()))
                })
                .cloned()
                .collect::<Vec<_>>();
            
            let total = results.len();
            let limit = request.limit.unwrap_or(20);
            let offset = request.offset.unwrap_or(0);
            
            let packages = results.into_iter()
                .skip(offset)
                .take(limit)
                .collect();
            
            Ok(NpmSearchResponse {
                packages,
                total,
                limit,
                offset,
            })
        }
        
        async fn get_dist_tags(&self, request: &NpmGetDistTagsRequest) -> Result<NpmGetDistTagsResponse, NpmReadError> {
            let mut dist_tags = std::collections::HashMap::new();
            
            // Simular dist-tags basados en las versiones disponibles
            let versions: Vec<String> = self.package_jsons.lock().unwrap()
                .keys()
                .filter(|k| k.starts_with(&format!("{}@", request.package_name.full_name())))
                .map(|k| k.split('@').last().unwrap().to_string())
                .collect();
            
            if !versions.is_empty() {
                dist_tags.insert("latest".to_string(), versions.last().unwrap().clone());
            }
            
            Ok(NpmGetDistTagsResponse {
                dist_tags,
                package_name: request.package_name.full_name().to_string(),
            })
        }
    }
    
    /// Mock para NpmPackageWriter
    pub struct MockNpmPackageWriter {
        pub packages: Mutex<HashMap<String, Vec<u8>>>,
        pub package_jsons: Mutex<HashMap<String, serde_json::Value>>,
        pub dist_tags: Mutex<HashMap<String, String>>,
    }
    
    impl MockNpmPackageWriter {
        pub fn new() -> Self {
            Self {
                packages: Mutex::new(HashMap::new()),
                package_jsons: Mutex::new(HashMap::new()),
                dist_tags: Mutex::new(HashMap::new()),
            }
        }
    }
    
    #[async_trait]
    impl NpmPackageWriter for MockNpmPackageWriter {
        async fn write_package(&self, request: &NpmPutPackageRequest) -> Result<NpmPutPackageResponse, NpmWriteError> {
            let key = format!("{}@{}", request.package_name.full_name(), request.version);
            
            // Verificar si ya existe y no se permite sobrescribir
            if !request.overwrite && self.packages.lock().unwrap().contains_key(&key) {
                return Err(NpmWriteError::PackageAlreadyExists {
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                });
            }
            
            // Guardar el paquete
            self.packages.lock().unwrap().insert(key.clone(), request.content.clone());
            
            // Si hay metadata, guardarla también
            if let Some(ref metadata) = request.metadata {
                self.package_jsons.lock().unwrap().insert(key.clone(), metadata.clone());
            }
            
            Ok(NpmPutPackageResponse {
                success: true,
                message: "Package published successfully".to_string(),
                package_name: request.package_name.full_name().to_string(),
                version: request.version.to_string(),
                tarball_url: format!("https://registry.npmjs.org/{}/-/{}/-/{}/{}.tgz", 
                    request.package_name.full_name(), 
                    request.package_name.package_name(), 
                    request.version, 
                    request.package_name.package_name()),
                size_bytes: request.content.len(),
                published_at: time::OffsetDateTime::now_utc(),
            })
        }
        
        async fn update_dist_tags(&self, request: &NpmUpdateDistTagsRequest) -> Result<NpmUpdateDistTagsResponse, NpmWriteError> {
            self.dist_tags.lock().unwrap().insert(request.tag.clone(), request.version.to_string());
            
            Ok(NpmUpdateDistTagsResponse {
                success: true,
                message: format!("Dist-tag {} updated to version {}", request.tag, request.version),
                package_name: request.package_name.full_name().to_string(),
                tag: request.tag.clone(),
                version: request.version.to_string(),
            })
        }
    }
    
    /// Mock para NpmRepositoryManager
    pub struct MockNpmRepositoryManager {
        pub repositories: Mutex<HashMap<String, NpmRepositoryInfo>>,
    }
    
    impl MockNpmRepositoryManager {
        pub fn new() -> Self {
            let mut repositories = HashMap::new();
            repositories.insert("npm-repo".to_string(), NpmRepositoryInfo {
                repository_id: "npm-repo".to_string(),
                name: "NPM Repository".to_string(),
                description: Some("Default npm repository".to_string()),
                is_public: true,
                allow_publish: true,
                registry_url: "https://registry.npmjs.org/".to_string(),
                max_package_size: Some(50 * 1024 * 1024), // 50MB
                supported_formats: vec!["tgz".to_string(), "tar.gz".to_string()],
            });
            
            Self {
                repositories: Mutex::new(repositories),
            }
        }
    }
    
    #[async_trait]
    impl NpmRepositoryManager for MockNpmRepositoryManager {
        async fn repository_exists(&self, repository_id: &str) -> Result<bool, NpmReadError> {
            Ok(self.repositories.lock().unwrap().contains_key(repository_id))
        }
        
        async fn get_repository_info(&self, repository_id: &str) -> Result<NpmRepositoryInfo, NpmReadError> {
            self.repositories.lock().unwrap()
                .get(repository_id)
                .cloned()
                .ok_or_else(|| NpmReadError::RepositoryNotFound {
                    repository_id: repository_id.to_string(),
                })
        }
        
        async fn can_publish(&self, repository_id: &str) -> Result<bool, NpmWriteError> {
            self.repositories.lock().unwrap()
                .get(repository_id)
                .map(|info| info.allow_publish)
                .ok_or_else(|| NpmWriteError::RepositoryNotFound {
                    repository_id: repository_id.to_string(),
                })
        }
        
        async fn get_repository_base_url(&self, repository_id: &str) -> Result<String, NpmReadError> {
            self.repositories.lock().unwrap()
                .get(repository_id)
                .map(|info| info.registry_url.clone())
                .ok_or_else(|| NpmReadError::RepositoryNotFound {
                    repository_id: repository_id.to_string(),
                })
        }
    }
    
    /// Mock para NpmPermissionChecker
    pub struct MockNpmPermissionChecker {
        pub read_permissions: Mutex<HashMap<String, bool>>,
        pub write_permissions: Mutex<HashMap<String, bool>>,
        pub dist_tag_permissions: Mutex<HashMap<String, bool>>,
    }
    
    impl MockNpmPermissionChecker {
        pub fn new() -> Self {
            Self {
                read_permissions: Mutex::new(HashMap::new()),
                write_permissions: Mutex::new(HashMap::new()),
                dist_tag_permissions: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn set_read_permission(&self, key: String, allowed: bool) {
            self.read_permissions.lock().unwrap().insert(key, allowed);
        }
        
        pub fn set_write_permission(&self, key: String, allowed: bool) {
            self.write_permissions.lock().unwrap().insert(key, allowed);
        }
        
        pub fn set_dist_tag_permission(&self, key: String, allowed: bool) {
            self.dist_tag_permissions.lock().unwrap().insert(key, allowed);
        }
    }
    
    #[async_trait]
    impl NpmPermissionChecker for MockNpmPermissionChecker {
        async fn can_read_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmReadError> {
            let key = format!("{}:{}:{}", user_id, repository_id, package_name.full_name());
            Ok(self.read_permissions.lock().unwrap().get(&key).copied().unwrap_or(true))
        }
        
        async fn can_write_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError> {
            let key = format!("{}:{}:{}", user_id, repository_id, package_name.full_name());
            Ok(self.write_permissions.lock().unwrap().get(&key).copied().unwrap_or(true))
        }
        
        async fn can_update_dist_tags(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError> {
            let key = format!("{}:{}:{}", user_id, repository_id, package_name.full_name());
            Ok(self.dist_tag_permissions.lock().unwrap().get(&key).copied().unwrap_or(true))
        }
    }
}