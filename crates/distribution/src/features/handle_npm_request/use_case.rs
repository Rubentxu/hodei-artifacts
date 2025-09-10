// crates/distribution/src/features/handle_npm_request/use_case.rs

//! Casos de uso para el feature Handle NPM Request
//! 
//! Lógica de negocio pura con validaciones exhaustivas y tracing estructurado.

use std::sync::Arc;
use tracing::{info, warn, error, instrument, Span};
use crate::domain::npm::{NpmPackageName, NpmVersion, validate_npm_package_name, validate_npm_version};
use super::ports::{
    NpmPackageReader, NpmPackageWriter, NpmRepositoryManager, NpmPermissionChecker,
    NpmReadError, NpmWriteError,
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

/// Caso de uso para obtener un paquete npm (.tgz)
pub struct HandleNpmGetPackageUseCase {
    package_reader: Arc<dyn NpmPackageReader>,
    repository_manager: Arc<dyn NpmRepositoryManager>,
    permission_checker: Arc<dyn NpmPermissionChecker>,
}

impl HandleNpmGetPackageUseCase {
    pub fn new(
        package_reader: Arc<dyn NpmPackageReader>,
        repository_manager: Arc<dyn NpmRepositoryManager>,
        permission_checker: Arc<dyn NpmPermissionChecker>,
    ) -> Self {
        Self {
            package_reader,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(
        name = "npm.get_package",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            repository.id = %request.repository_id,
            user.id = "system" // TODO: obtener del contexto
        )
    )]
    pub async fn execute(&self, request: NpmGetPackageRequest) -> Result<NpmGetPackageResponse, NpmReadError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            repository_id = %request.repository_id,
            "Processing npm package download request"
        );
        
        // 1. Validar el request
        self.validate_request(&request)?;
        
        // 2. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            error!(
                repository_id = %request.repository_id,
                "Repository not found"
            );
            return Err(NpmReadError::RepositoryNotFound {
                repository_id: request.repository_id.clone(),
            });
        }
        
        // 3. Verificar permisos de lectura
        let user_id = "system"; // TODO: obtener del contexto de autenticación
        if !self.permission_checker.can_read_package(user_id, &request.repository_id, &request.package_name).await? {
            error!(
                user_id = %user_id,
                package_name = %request.package_name.full_name(),
                "Permission denied for package read"
            );
            return Err(NpmReadError::PermissionDenied {
                package_name: request.package_name.full_name().to_string(),
            });
        }
        
        // 4. Leer el paquete
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            "Reading npm package"
        );
        
        let response = self.package_reader.read_package(&request).await?;
        
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            content_length = response.content_length,
            "Successfully read npm package"
        );
        
        Ok(response)
    }
    
    fn validate_request(&self, request: &NpmGetPackageRequest) -> Result<(), NpmReadError> {
        // Validar nombre del paquete
        validate_npm_package_name(request.package_name.full_name())
            .map_err(|e| NpmReadError::InvalidPackageName(e.to_string()))?;
        
        // Validar versión
        validate_npm_version(&request.version.to_string())
            .map_err(|e| NpmReadError::InvalidVersion(e.to_string()))?;
        
        Ok(())
    }
}

/// Caso de uso para publicar un paquete npm (.tgz)
pub struct HandleNpmPutPackageUseCase {
    package_writer: Arc<dyn NpmPackageWriter>,
    repository_manager: Arc<dyn NpmRepositoryManager>,
    permission_checker: Arc<dyn NpmPermissionChecker>,
}

impl HandleNpmPutPackageUseCase {
    pub fn new(
        package_writer: Arc<dyn NpmPackageWriter>,
        repository_manager: Arc<dyn NpmRepositoryManager>,
        permission_checker: Arc<dyn NpmPermissionChecker>,
    ) -> Self {
        Self {
            package_writer,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(
        name = "npm.put_package",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            repository.id = %request.repository_id,
            content.length = request.content.len(),
            overwrite = request.overwrite,
            user.id = "system" // TODO: obtener del contexto
        )
    )]
    pub async fn execute(&self, request: NpmPutPackageRequest) -> Result<NpmPutPackageResponse, NpmWriteError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            repository_id = %request.repository_id,
            content_length = request.content.len(),
            overwrite = request.overwrite,
            "Processing npm package publish request"
        );
        
        // 1. Validar el request
        self.validate_request(&request)?;
        
        // 2. Verificar que el repositorio permite publicación
        if !self.repository_manager.can_publish(&request.repository_id).await? {
            error!(
                repository_id = %request.repository_id,
                "Repository does not allow publishing"
            );
            return Err(NpmWriteError::RepositoryNotFound {
                repository_id: request.repository_id.clone(),
            });
        }
        
        // 3. Verificar permisos de escritura
        let user_id = "system"; // TODO: obtener del contexto de autenticación
        if !self.permission_checker.can_write_package(user_id, &request.repository_id, &request.package_name).await? {
            error!(
                user_id = %user_id,
                package_name = %request.package_name.full_name(),
                "Permission denied for package write"
            );
            return Err(NpmWriteError::PermissionDenied {
                package_name: request.package_name.full_name().to_string(),
            });
        }
        
        // 4. Validar el contenido del paquete
        self.validate_package_content(&request)?;
        
        // 5. Escribir el paquete
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            "Publishing npm package"
        );
        
        let response = self.package_writer.write_package(&request).await?;
        
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            size_bytes = response.size_bytes,
            published_at = ?response.published_at,
            "Successfully published npm package"
        );
        
        Ok(response)
    }
    
    fn validate_request(&self, request: &NpmPutPackageRequest) -> Result<(), NpmWriteError> {
        // Validar nombre del paquete
        validate_npm_package_name(request.package_name.full_name())
            .map_err(|e| NpmWriteError::InvalidPackageName(e.to_string()))?;
        
        // Validar versión
        validate_npm_version(&request.version.to_string())
            .map_err(|e| NpmWriteError::InvalidVersion(e.to_string()))?;
        
        // Validar tipo de contenido
        if request.content_type != "application/octet-stream" && request.content_type != "application/gzip" {
            return Err(NpmWriteError::InvalidPackageContent(
                format!("Invalid content type: {}", request.content_type)
            ));
        }
        
        // Validar tamaño del contenido (máximo 50MB por defecto)
        if request.content.len() > 50 * 1024 * 1024 {
            return Err(NpmWriteError::InvalidPackageContent(
                format!("Package too large: {} bytes (max 50MB)", request.content.len())
            ));
        }
        
        Ok(())
    }
    
    fn validate_package_content(&self, request: &NpmPutPackageRequest) -> Result<(), NpmWriteError> {
        // Validar que el contenido parezca un tarball válido
        if request.content.is_empty() {
            return Err(NpmWriteError::InvalidPackageContent(
                "Package content cannot be empty".to_string()
            ));
        }
        
        // TODO: Validar que sea un tarball npm válido
        // - Verificar la estructura del tarball
        // - Validar que contenga un package.json válido
        // - Verificar integridad del contenido
        
        Ok(())
    }
}

/// Caso de uso para verificar existencia de un paquete npm (HEAD)
pub struct HandleNpmHeadPackageUseCase {
    package_reader: Arc<dyn NpmPackageReader>,
    repository_manager: Arc<dyn NpmRepositoryManager>,
    permission_checker: Arc<dyn NpmPermissionChecker>,
}

impl HandleNpmHeadPackageUseCase {
    pub fn new(
        package_reader: Arc<dyn NpmPackageReader>,
        repository_manager: Arc<dyn NpmRepositoryManager>,
        permission_checker: Arc<dyn NpmPermissionChecker>,
    ) -> Self {
        Self {
            package_reader,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(
        name = "npm.head_package",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = %request.version,
            repository.id = %request.repository_id,
            user.id = "system" // TODO: obtener del contexto
        )
    )]
    pub async fn execute(&self, request: NpmHeadPackageRequest) -> Result<NpmHeadPackageResponse, NpmReadError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            repository_id = %request.repository_id,
            "Processing npm package head request"
        );
        
        // 1. Validar el request
        self.validate_request(&request)?;
        
        // 2. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            error!(
                repository_id = %request.repository_id,
                "Repository not found"
            );
            return Err(NpmReadError::RepositoryNotFound {
                repository_id: request.repository_id.clone(),
            });
        }
        
        // 3. Verificar permisos de lectura
        let user_id = "system"; // TODO: obtener del contexto de autenticación
        if !self.permission_checker.can_read_package(user_id, &request.repository_id, &request.package_name).await? {
            error!(
                user_id = %user_id,
                package_name = %request.package_name.full_name(),
                "Permission denied for package read"
            );
            return Err(NpmReadError::PermissionDenied {
                package_name: request.package_name.full_name().to_string(),
            });
        }
        
        // 4. Verificar existencia del paquete
        let exists = self.package_reader.package_exists(&request).await?;
        
        info!(
            package_name = %request.package_name.full_name(),
            version = %request.version,
            exists = exists,
            "Completed npm package head check"
        );
        
        Ok(NpmHeadPackageResponse {
            exists,
            content_length: None, // TODO: obtener del storage si existe
            last_modified: None,
            etag: None,
            package_name: request.package_name.full_name().to_string(),
            version: request.version.to_string(),
        })
    }
    
    fn validate_request(&self, request: &NpmHeadPackageRequest) -> Result<(), NpmReadError> {
        // Validar nombre del paquete
        validate_npm_package_name(request.package_name.full_name())
            .map_err(|e| NpmReadError::InvalidPackageName(e.to_string()))?;
        
        // Validar versión
        validate_npm_version(&request.version.to_string())
            .map_err(|e| NpmReadError::InvalidVersion(e.to_string()))?;
        
        Ok(())
    }
}

/// Caso de uso para obtener metadata de un paquete (package.json)
pub struct HandleNpmGetPackageJsonUseCase {
    package_reader: Arc<dyn NpmPackageReader>,
    repository_manager: Arc<dyn NpmRepositoryManager>,
    permission_checker: Arc<dyn NpmPermissionChecker>,
}

impl HandleNpmGetPackageJsonUseCase {
    pub fn new(
        package_reader: Arc<dyn NpmPackageReader>,
        repository_manager: Arc<dyn NpmRepositoryManager>,
        permission_checker: Arc<dyn NpmPermissionChecker>,
    ) -> Self {
        Self {
            package_reader,
            repository_manager,
            permission_checker,
        }
    }
    
    #[instrument(
        name = "npm.get_package_json",
        skip(self, request),
        fields(
            package.name = %request.package_name.full_name(),
            package.version = ?request.version,
            repository.id = %request.repository_id,
            user.id = "system" // TODO: obtener del contexto
        )
    )]
    pub async fn execute(&self, request: NpmGetPackageJsonRequest) -> Result<NpmGetPackageJsonResponse, NpmReadError> {
        info!(
            package_name = %request.package_name.full_name(),
            version = ?request.version,
            repository_id = %request.repository_id,
            "Processing npm package.json request"
        );
        
        // 1. Validar el request
        self.validate_request(&request)?;
        
        // 2. Verificar que el repositorio existe
        if !self.repository_manager.repository_exists(&request.repository_id).await? {
            error!(
                repository_id = %request.repository_id,
                "Repository not found"
            );
            return Err(NpmReadError::RepositoryNotFound {
                repository_id: request.repository_id.clone(),
            });
        }
        
        // 3. Verificar permisos de lectura
        let user_id = "system"; // TODO: obtener del contexto de autenticación
        if !self.permission_checker.can_read_package(user_id, &request.repository_id, &request.package_name).await? {
            error!(
                user_id = %user_id,
                package_name = %request.package_name.full_name(),
                "Permission denied for package read"
            );
            return Err(NpmReadError::PermissionDenied {
                package_name: request.package_name.full_name().to_string(),
            });
        }
        
        // 4. Leer el package.json
        info!(
            package_name = %request.package_name.full_name(),
            version = ?request.version,
            "Reading npm package.json"
        );
        
        let response = self.package_reader.read_package_json(&request).await?;
        
        info!(
            package_name = %request.package_name.full_name(),
            version = %response.version,
            content_length = response.content_length,
            "Successfully read npm package.json"
        );
        
        Ok(response)
    }
    
    fn validate_request(&self, request: &NpmGetPackageJsonRequest) -> Result<(), NpmReadError> {
        // Validar nombre del paquete
        validate_npm_package_name(request.package_name.full_name())
            .map_err(|e| NpmReadError::InvalidPackageName(e.to_string()))?;
        
        // Validar versión si se proporciona
        if let Some(ref version) = request.version {
            validate_npm_version(&version.to_string())
                .map_err(|e| NpmReadError::InvalidVersion(e.to_string()))?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::{NpmPackageName, NpmVersion};
    use crate::features::handle_npm_request::ports::test::{
        MockNpmPackageReader, MockNpmPackageWriter, MockNpmRepositoryManager, MockNpmPermissionChecker,
    };
    
    #[tokio::test]
    async fn test_handle_npm_get_package_success() {
        let package_reader = Arc::new(MockNpmPackageReader::new());
        let repository_manager = Arc::new(MockNpmRepositoryManager::new());
        let permission_checker = Arc::new(MockNpmPermissionChecker::new());
        
        let use_case = HandleNpmGetPackageUseCase::new(
            package_reader.clone(),
            repository_manager,
            permission_checker,
        );
        
        let name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmGetPackageRequest {
            package_name: name.clone(),
            version: version.clone(),
            repository_id: "npm-repo".to_string(),
        };
        
        let response = use_case.execute(request).await.unwrap();
        
        assert_eq!(response.package_name, "test-package");
        assert_eq!(response.version, "1.0.0");
    }
    
    #[tokio::test]
    async fn test_handle_npm_put_package_success() {
        let package_writer = Arc::new(MockNpmPackageWriter::new());
        let repository_manager = Arc::new(MockNpmRepositoryManager::new());
        let permission_checker = Arc::new(MockNpmPermissionChecker::new());
        
        let use_case = HandleNpmPutPackageUseCase::new(
            package_writer,
            repository_manager,
            permission_checker,
        );
        
        let name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        
        let request = NpmPutPackageRequest {
            package_name: name.clone(),
            version: version.clone(),
            content: b"test package content".to_vec(),
            content_type: "application/octet-stream".to_string(),
            repository_id: "npm-repo".to_string(),
            overwrite: false,
            metadata: None,
        };
        
        let response = use_case.execute(request).await.unwrap();
        
        assert!(response.success);
        assert_eq!(response.package_name, "test-package");
        assert_eq!(response.version, "1.0.0");
    }
}