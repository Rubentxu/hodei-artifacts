// crates/distribution/src/features/handle_npm_request/adapter.rs

//! Adaptadores de infraestructura para el feature Handle NPM Request
//! 
//! Implementaciones concretas de los puertos definidos en este feature.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use crate::domain::npm::{NpmPackageName, NpmVersion, NpmPackageMetadata, NpmRepositoryMetadata};
use super::ports::{
    NpmPackageReader, NpmPackageWriter, NpmRepositoryManager, NpmPermissionChecker,
    NpmReadError, NpmWriteError, NpmRepositoryInfo,
};
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

/// Adaptador de producción para leer paquetes npm desde S3
pub struct S3NpmPackageReader {
    s3_client: Arc<dyn S3Client>,
    bucket_name: String,
    base_path: String,
}

impl S3NpmPackageReader {
    pub fn new(s3_client: Arc<dyn S3Client>, bucket_name: String, base_path: String) -> Self {
        Self {
            s3_client,
            bucket_name,
            base_path,
        }
    }
    
    fn get_package_key(&self, package_name: &NpmPackageName, version: &NpmVersion) -> String {
        format!("{}/npm/{}/-/{}-{}.tgz", 
            self.base_path, 
            package_name.full_name(), 
            package_name.package_name(), 
            version)
    }
    
    fn get_package_json_key(&self, package_name: &NpmPackageName, version: &NpmVersion) -> String {
        format!("{}/npm/{}/-/{}-{}.json", 
            self.base_path, 
            package_name.full_name(), 
            package_name.package_name(), 
            version)
    }
}

#[async_trait]
impl NpmPackageReader for S3NpmPackageReader {
    #[instrument(
        name = "s3.npm.read_package",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            bucket = %self.bucket_name
        )
    )]
    async fn read_package(&self, request: &NpmGetPackageRequest) -> Result<NpmGetPackageResponse, NpmReadError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            "Reading npm package from S3"
        );
        
        let key = self.get_package_key(&request.package_name, &request.version);
        
        match self.s3_client.get_object(&self.bucket_name, &key).await {
            Ok(object_data) => {
                info!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    content_length = object_data.content.len(),
                    "Successfully read npm package from S3"
                );
                
                Ok(NpmGetPackageResponse {
                    content: object_data.content,
                    content_type: "application/octet-stream".to_string(),
                    content_length: object_data.content.len(),
                    last_modified: object_data.last_modified,
                    etag: object_data.etag,
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                    integrity: object_data.checksum.map(|c| format!("sha512-{}", c)),
                })
            }
            Err(S3Error::NotFound) => {
                warn!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    key = %key,
                    "Npm package not found in S3"
                );
                Err(NpmReadError::PackageNotFound {
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                })
            }
            Err(S3Error::PermissionDenied) => {
                error!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    "Permission denied reading npm package from S3"
                );
                Err(NpmReadError::PermissionDenied {
                    package_name: request.package_name.full_name().to_string(),
                })
            }
            Err(e) => {
                error!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    error = %e,
                    "Error reading npm package from S3"
                );
                Err(NpmReadError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }
    
    #[instrument(
        name = "s3.npm.package_exists",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            bucket = %self.bucket_name
        )
    )]
    async fn package_exists(&self, request: &NpmHeadPackageRequest) -> Result<bool, NpmReadError> {
        let key = self.get_package_key(&request.package_name, &request.version);
        
        match self.s3_client.object_exists(&self.bucket_name, &key).await {
            Ok(exists) => {
                info!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    exists = exists,
                    "Checked npm package existence in S3"
                );
                Ok(exists)
            }
            Err(e) => {
                error!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    error = %e,
                    "Error checking npm package existence in S3"
                );
                Err(NpmReadError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }
    
    #[instrument(
        name = "s3.npm.read_package_json",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = ?request.version,
            bucket = %self.bucket_name
        )
    )]
    async fn read_package_json(&self, request: &NpmGetPackageJsonRequest) -> Result<NpmGetPackageJsonResponse, NpmReadError> {
        let version = request.version.as_ref()
            .ok_or_else(|| NpmReadError::InvalidVersion("Version is required for package.json".to_string()))?;
        
        let key = self.get_package_json_key(&request.package_name, version);
        
        match self.s3_client.get_object(&self.bucket_name, &key).await {
            Ok(object_data) => {
                let package_json: serde_json::Value = serde_json::from_slice(&object_data.content)
                    .map_err(|e| NpmReadError::StorageError(format!("Invalid JSON in S3: {}", e)))?;
                
                info!(
                    package_name = %request.package_name.full_name(),
                    version = %version,
                    content_length = object_data.content.len(),
                    "Successfully read npm package.json from S3"
                );
                
                Ok(NpmGetPackageJsonResponse {
                    package_json,
                    content_type: "application/json".to_string(),
                    content_length: object_data.content.len(),
                    last_modified: object_data.last_modified,
                    etag: object_data.etag,
                    package_name: request.package_name.full_name().to_string(),
                    version: version.to_string(),
                })
            }
            Err(S3Error::NotFound) => {
                warn!(
                    package_name = %request.package_name.full_name(),
                    version = %version,
                    key = %key,
                    "Npm package.json not found in S3"
                );
                Err(NpmReadError::PackageNotFound {
                    package_name: request.package_name.full_name().to_string(),
                    version: version.to_string(),
                })
            }
            Err(e) => {
                error!(
                    package_name = %request.package_name.full_name(),
                    version = %version,
                    error = %e,
                    "Error reading npm package.json from S3"
                );
                Err(NpmReadError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }
    
    async fn read_repository_info(&self, request: &NpmGetRepositoryInfoRequest) -> Result<NpmGetRepositoryInfoResponse, NpmReadError> {
        // TODO: Implementar lectura de metadata del repositorio desde S3
        // Por ahora, devolver información básica
        let package_json = serde_json::json!({
            "name": request.package_name.full_name(),
            "versions": {},
            "dist-tags": {}
        });
        
        Ok(NpmGetRepositoryInfoResponse {
            package_json,
            content_type: "application/json".to_string(),
            content_length: 0,
            last_modified: None,
            etag: None,
            package_name: request.package_name.full_name().to_string(),
            versions: Vec::new(),
            dist_tags: std::collections::HashMap::new(),
        })
    }
    
    async fn search_packages(&self, request: &NpmSearchRequest) -> Result<NpmSearchResponse, NpmReadError> {
        // TODO: Implementar búsqueda en S3
        // Por ahora, devolver resultados vacíos
        Ok(NpmSearchResponse {
            packages: Vec::new(),
            total: 0,
            limit: request.limit.unwrap_or(20),
            offset: request.offset.unwrap_or(0),
        })
    }
    
    async fn get_dist_tags(&self, request: &NpmGetDistTagsRequest) -> Result<NpmGetDistTagsResponse, NpmReadError> {
        // TODO: Implementar lectura de dist-tags desde S3
        // Por ahora, devolver dist-tags vacíos
        Ok(NpmGetDistTagsResponse {
            dist_tags: std::collections::HashMap::new(),
            package_name: request.package_name.full_name().to_string(),
        })
    }
}

/// Adaptador de producción para escribir paquetes npm a S3
pub struct S3NpmPackageWriter {
    s3_client: Arc<dyn S3Client>,
    bucket_name: String,
    base_path: String,
}

impl S3NpmPackageWriter {
    pub fn new(s3_client: Arc<dyn S3Client>, bucket_name: String, base_path: String) -> Self {
        Self {
            s3_client,
            bucket_name,
            base_path,
        }
    }
    
    fn get_package_key(&self, package_name: &NpmPackageName, version: &NpmVersion) -> String {
        format!("{}/npm/{}/-/{}-{}.tgz", 
            self.base_path, 
            package_name.full_name(), 
            package_name.package_name(), 
            version)
    }
    
    fn get_package_json_key(&self, package_name: &NpmPackageName, version: &NpmVersion) -> String {
        format!("{}/npm/{}/-/{}-{}.json", 
            self.base_path, 
            package_name.full_name(), 
            package_name.package_name(), 
            version)
    }
}

#[async_trait]
impl NpmPackageWriter for S3NpmPackageWriter {
    #[instrument(
        name = "s3.npm.write_package",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            content.length = request.content.len(),
            bucket = %self.bucket_name
        )
    )]
    async fn write_package(&self, request: &NpmPutPackageRequest) -> Result<NpmPutPackageResponse, NpmWriteError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            content_length = request.content.len(),
            overwrite = request.overwrite,
            "Writing npm package to S3"
        );
        
        let key = self.get_package_key(&request.package_name, &request.version);
        
        // Verificar si ya existe
        if !request.overwrite {
            match self.s3_client.object_exists(&self.bucket_name, &key).await {
                Ok(true) => {
                    warn!(
                        package_name = %request.package_name.full_name(),
                        version = %request.version,
                        "Package already exists in S3"
                    );
                    return Err(NpmWriteError::PackageAlreadyExists {
                        package_name: request.package_name.full_name().to_string(),
                        version: request.version.to_string(),
                    });
                }
                Ok(false) => {
                    // No existe, continuar
                }
                Err(e) => {
                    error!(
                        package_name = %request.package_name.full_name(),
                        version = %request.version,
                        error = %e,
                        "Error checking package existence in S3"
                    );
                    return Err(NpmWriteError::StorageError(format!("S3 error: {}", e)));
                }
            }
        }
        
        // Escribir el paquete
        match self.s3_client.put_object(
            &self.bucket_name,
            &key,
            &request.content,
            &request.content_type,
        ).await {
            Ok(()) => {
                info!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    key = %key,
                    "Successfully wrote npm package to S3"
                );
                
                // Si hay metadata, también escribir el package.json
                if let Some(ref metadata) = request.metadata {
                    let json_key = self.get_package_json_key(&request.package_name, &request.version);
                    let json_content = serde_json::to_vec(metadata)
                        .map_err(|e| NpmWriteError::InvalidPackageContent(format!("Invalid JSON: {}", e)))?;
                    
                    if let Err(e) = self.s3_client.put_object(
                        &self.bucket_name,
                        &json_key,
                        &json_content,
                        "application/json",
                    ).await {
                        error!(
                            package_name = %request.package_name.full_name(),
                            version = %request.version,
                            error = %e,
                            "Error writing package.json to S3"
                        );
                        // No fallar si no podemos escribir el JSON, pero loggear el error
                    }
                }
                
                Ok(NpmPutPackageResponse {
                    success: true,
                    message: "Package published successfully".to_string(),
                    package_name: request.package_name.full_name().to_string(),
                    version: request.version.to_string(),
                    tarball_url: format!("https://{}/{}/{}", 
                        self.s3_client.get_endpoint(), 
                        self.bucket_name, 
                        key),
                    size_bytes: request.content.len(),
                    published_at: time::OffsetDateTime::now_utc(),
                })
            }
            Err(e) => {
                error!(
                    package_name = %request.package_name.full_name(),
                    version = %request.version,
                    error = %e,
                    "Error writing npm package to S3"
                );
                Err(NpmWriteError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }
    
    #[instrument(
        name = "s3.npm.update_dist_tags",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            tag = %request.tag,
            version = %request.version,
            bucket = %self.bucket_name
        )
    )]
    async fn update_dist_tags(&self, request: &NpmUpdateDistTagsRequest) -> Result<NpmUpdateDistTagsResponse, NpmWriteError> {
        info!(
            package_name = %request.package_name.full_name(),
            tag = %request.tag,
            version = %request.version,
            "Updating npm dist-tags in S3"
        );
        
        // TODO: Implementar actualización de dist-tags en S3
        // Por ahora, simplemente devolver éxito
        
        Ok(NpmUpdateDistTagsResponse {
            success: true,
            message: format!("Dist-tag {} updated to version {}", request.tag, request.version),
            package_name: request.package_name.full_name().to_string(),
            tag: request.tag.clone(),
            version: request.version.to_string(),
        })
    }
}

/// Adaptador de producción para gestión de repositorios npm
pub struct MongoNpmRepositoryManager {
    mongo_client: Arc<dyn MongoClient>,
    database_name: String,
}

impl MongoNpmRepositoryManager {
    pub fn new(mongo_client: Arc<dyn MongoClient>, database_name: String) -> Self {
        Self {
            mongo_client,
            database_name,
        }
    }
}

#[async_trait]
impl NpmRepositoryManager for MongoNpmRepositoryManager {
    #[instrument(
        name = "mongo.npm.repository_exists",
        skip(self),
        fields(
            repository.id = %repository_id
        )
    )]
    async fn repository_exists(&self, repository_id: &str) -> Result<bool, NpmReadError> {
        // TODO: Implementar verificación de existencia de repositorio en MongoDB
        info!(
            repository_id = %repository_id,
            "Checking npm repository existence in MongoDB"
        );
        
        // Por ahora, asumir que existe
        Ok(true)
    }
    
    #[instrument(
        name = "mongo.npm.get_repository_info",
        skip(self),
        fields(
            repository.id = %repository_id
        )
    )]
    async fn get_repository_info(&self, repository_id: &str) -> Result<NpmRepositoryInfo, NpmReadError> {
        // TODO: Implementar lectura de información del repositorio desde MongoDB
        info!(
            repository_id = %repository_id,
            "Getting npm repository info from MongoDB"
        );
        
        Ok(NpmRepositoryInfo {
            repository_id: repository_id.to_string(),
            name: "NPM Repository".to_string(),
            description: Some("Default npm repository".to_string()),
            is_public: true,
            allow_publish: true,
            registry_url: "https://registry.npmjs.org/".to_string(),
            max_package_size: Some(50 * 1024 * 1024), // 50MB
            supported_formats: vec!["tgz".to_string(), "tar.gz".to_string()],
        })
    }
    
    #[instrument(
        name = "mongo.npm.can_publish",
        skip(self),
        fields(
            repository.id = %repository_id
        )
    )]
    async fn can_publish(&self, repository_id: &str) -> Result<bool, NpmWriteError> {
        // TODO: Implementar verificación de permisos de publicación desde MongoDB
        info!(
            repository_id = %repository_id,
            "Checking npm repository publish permissions in MongoDB"
        );
        
        Ok(true)
    }
    
    #[instrument(
        name = "mongo.npm.get_repository_base_url",
        skip(self),
        fields(
            repository.id = %repository_id
        )
    )]
    async fn get_repository_base_url(&self, repository_id: &str) -> Result<String, NpmReadError> {
        // TODO: Implementar lectura de URL base del repositorio desde MongoDB
        info!(
            repository_id = %repository_id,
            "Getting npm repository base URL from MongoDB"
        );
        
        Ok("https://registry.npmjs.org/".to_string())
    }
}

/// Adaptador de producción para control de permisos npm con Cedar
pub struct CedarNpmPermissionChecker {
    cedar_engine: Arc<dyn CedarEngine>,
}

impl CedarNpmPermissionChecker {
    pub fn new(cedar_engine: Arc<dyn CedarEngine>) -> Self {
        Self {
            cedar_engine,
        }
    }
}

#[async_trait]
impl NpmPermissionChecker for CedarNpmPermissionChecker {
    #[instrument(
        name = "cedar.npm.can_read_package",
        skip(self),
        fields(
            user.id = %user_id,
            repository.id = %repository_id,
            package.name = %package_name.full_name()
        )
    )]
    async fn can_read_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmReadError> {
        info!(
            user_id = %user_id,
            repository_id = %repository_id,
            package_name = %package_name.full_name(),
            "Checking npm package read permission with Cedar"
        );
        
        // TODO: Implementar evaluación de políticas Cedar
        // Por ahora, permitir lectura pública
        Ok(true)
    }
    
    #[instrument(
        name = "cedar.npm.can_write_package",
        skip(self),
        fields(
            user.id = %user_id,
            repository.id = %repository_id,
            package.name = %package_name.full_name()
        )
    )]
    async fn can_write_package(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError> {
        info!(
            user_id = %user_id,
            repository_id = %repository_id,
            package_name = %package_name.full_name(),
            "Checking npm package write permission with Cedar"
        );
        
        // TODO: Implementar evaluación de políticas Cedar
        // Por ahora, permitir escritura a usuarios autenticados
        Ok(!user_id.is_empty() && user_id != "anonymous")
    }
    
    #[instrument(
        name = "cedar.npm.can_update_dist_tags",
        skip(self),
        fields(
            user.id = %user_id,
            repository.id = %repository_id,
            package.name = %package_name.full_name()
        )
    )]
    async fn can_update_dist_tags(&self, user_id: &str, repository_id: &str, package_name: &NpmPackageName) -> Result<bool, NpmWriteError> {
        info!(
            user_id = %user_id,
            repository_id = %repository_id,
            package_name = %package_name.full_name(),
            "Checking npm dist-tags update permission with Cedar"
        );
        
        // TODO: Implementar evaluación de políticas Cedar
        // Por ahora, requerir permisos de administrador
        Ok(user_id == "admin" || user_id.ends_with(":admin"))
    }
}

/// Trait para cliente S3 (para testing y mocking)
#[async_trait]
pub trait S3Client: Send + Sync {
    async fn get_object(&self, bucket: &str, key: &str) -> Result<S3ObjectData, S3Error>;
    async fn put_object(&self, bucket: &str, key: &str, content: &[u8], content_type: &str) -> Result<(), S3Error>;
    async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, S3Error>;
    fn get_endpoint(&self) -> String;
}

/// Datos de objeto S3
#[derive(Debug, Clone)]
pub struct S3ObjectData {
    pub content: Vec<u8>,
    pub content_type: String,
    pub last_modified: Option<time::OffsetDateTime>,
    pub etag: Option<String>,
    pub checksum: Option<String>,
}

/// Errores de S3
#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("Object not found")]
    NotFound,
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Storage error: {0}")]
    StorageError(String),
}

/// Trait para cliente MongoDB (para testing y mocking)
#[async_trait]
pub trait MongoClient: Send + Sync {
    // TODO: Agregar métodos específicos para MongoDB
}

/// Trait para motor Cedar (para testing y mocking)
#[async_trait]
pub trait CedarEngine: Send + Sync {
    // TODO: Agregar métodos específicos para Cedar
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    
    /// Mock S3 client para testing
    pub struct MockS3Client {
        objects: Mutex<HashMap<String, S3ObjectData>>,
    }
    
    impl MockS3Client {
        pub fn new() -> Self {
            Self {
                objects: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_object(&self, bucket: &str, key: &str, data: S3ObjectData) {
            let full_key = format!("{}:{}", bucket, key);
            self.objects.lock().unwrap().insert(full_key, data);
        }
    }
    
    #[async_trait]
    impl S3Client for MockS3Client {
        async fn get_object(&self, bucket: &str, key: &str) -> Result<S3ObjectData, S3Error> {
            let full_key = format!("{}:{}", bucket, key);
            self.objects.lock().unwrap()
                .get(&full_key)
                .cloned()
                .ok_or(S3Error::NotFound)
        }
        
        async fn put_object(&self, bucket: &str, key: &str, content: &[u8], content_type: &str) -> Result<(), S3Error> {
            let full_key = format!("{}:{}", bucket, key);
            let data = S3ObjectData {
                content: content.to_vec(),
                content_type: content_type.to_string(),
                last_modified: Some(time::OffsetDateTime::now_utc()),
                etag: Some(format!("\"{}\"", full_key)),
                checksum: None,
            };
            self.objects.lock().unwrap().insert(full_key, data);
            Ok(())
        }
        
        async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, S3Error> {
            let full_key = format!("{}:{}", bucket, key);
            Ok(self.objects.lock().unwrap().contains_key(&full_key))
        }
        
        fn get_endpoint(&self) -> String {
            "s3.amazonaws.com".to_string()
        }
    }
    
    #[tokio::test]
    async fn test_s3_npm_package_reader() {
        let s3_client = Arc::new(MockS3Client::new());
        let reader = S3NpmPackageReader::new(
            s3_client.clone(),
            "test-bucket".to_string(),
            "artifacts".to_string(),
        );
        
        // Agregar un paquete mock
        let package_data = S3ObjectData {
            content: b"test package content".to_vec(),
            content_type: "application/octet-stream".to_string(),
            last_modified: Some(time::OffsetDateTime::now_utc()),
            etag: Some("\"test-etag\"".to_string()),
            checksum: Some("abc123".to_string()),
        };
        
        s3_client.add_object("test-bucket", "artifacts/npm/test-package/-/test-package-1.0.0.tgz", package_data);
        
        let name = crate::domain::npm::NpmPackageName::new("test-package").unwrap();
        let version = crate::domain::npm::NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmGetPackageRequest {
            package_name: name,
            version,
            repository_id: "npm-repo".to_string(),
        };
        
        let response = reader.read_package(&request).await.unwrap();
        
        assert_eq!(response.content, b"test package content");
        assert_eq!(response.content_type, "application/octet-stream");
        assert_eq!(response.package_name, "test-package");
        assert_eq!(response.version, "1.0.0");
        assert_eq!(response.integrity, Some("sha512-abc123".to_string()));
    }
    
    #[tokio::test]
    async fn test_s3_npm_package_writer() {
        let s3_client = Arc::new(MockS3Client::new());
        let writer = S3NpmPackageWriter::new(
            s3_client.clone(),
            "test-bucket".to_string(),
            "artifacts".to_string(),
        );
        
        let name = crate::domain::npm::NpmPackageName::new("test-package").unwrap();
        let version = crate::domain::npm::NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmPutPackageRequest {
            package_name: name.clone(),
            version: version.clone(),
            content: b"new package content".to_vec(),
            content_type: "application/octet-stream".to_string(),
            repository_id: "npm-repo".to_string(),
            overwrite: false,
            metadata: None,
        };
        
        let response = writer.write_package(&request).await.unwrap();
        
        assert!(response.success);
        assert_eq!(response.package_name, "test-package");
        assert_eq!(response.version, "1.0.0");
        assert_eq!(response.size_bytes, 19);
        
        // Verificar que el objeto fue creado
        let full_key = format!("test-bucket:artifacts/npm/test-package/-/test-package-1.0.0.tgz");
        assert!(s3_client.objects.lock().unwrap().contains_key(&full_key));
    }
}