// crates/distribution/src/features/handle_npm_request/api.rs

//! API endpoint para el feature Handle NPM Request
//! 
//! Punto de entrada HTTP que maneja todas las operaciones npm:
//! - GET /npm/{package}/-/{filename} - Descargar paquete (.tgz)
//! - PUT /npm/{package}/-/{filename} - Publicar paquete (.tgz)
//! - HEAD /npm/{package}/-/{filename} - Verificar existencia
//! - GET /npm/{package} - Obtener metadata del repositorio
//! - GET /npm/{package}/{version} - Obtener package.json específico

use axum::{
    extract::{Path, State, Extension},
    http::{StatusCode, HeaderMap, HeaderValue},
    response::{Response, IntoResponse},
    body::Body,
};
use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use crate::domain::npm::{NpmPackageName, NpmVersion};
use super::use_case::{
    HandleNpmGetPackageUseCase, HandleNpmPutPackageUseCase, 
    HandleNpmHeadPackageUseCase, HandleNpmGetPackageJsonUseCase,
};
use super::dto::{
    NpmGetPackageRequest, NpmPutPackageRequest, NpmHeadPackageRequest,
    NpmGetPackageJsonRequest, NpmGetRepositoryInfoRequest,
};

/// Estado compartido del API endpoint
#[derive(Clone)]
pub struct NpmRequestHandler {
    get_package_use_case: Arc<HandleNpmGetPackageUseCase>,
    put_package_use_case: Arc<HandleNpmPutPackageUseCase>,
    head_package_use_case: Arc<HandleNpmHeadPackageUseCase>,
    get_package_json_use_case: Arc<HandleNpmGetPackageJsonUseCase>,
}

impl NpmRequestHandler {
    pub fn new(
        get_package_use_case: Arc<HandleNpmGetPackageUseCase>,
        put_package_use_case: Arc<HandleNpmPutPackageUseCase>,
        head_package_use_case: Arc<HandleNpmHeadPackageUseCase>,
        get_package_json_use_case: Arc<HandleNpmGetPackageJsonUseCase>,
    ) -> Self {
        Self {
            get_package_use_case,
            put_package_use_case,
            head_package_use_case,
            get_package_json_use_case,
        }
    }
    
    /// Manejar GET request para descargar un paquete npm (.tgz)
    #[instrument(
        name = "npm.api.get_package",
        skip(self, headers),
        fields(
            package.name = %package_name,
            package.version = %version,
            filename = %filename
        )
    )]
    pub async fn handle_get_package(
        &self,
        Path((package_name, version, filename)): Path<(String, String, String)>,
        headers: HeaderMap,
        Extension(repository_id): Extension<String>,
    ) -> Result<Response<Body>, NpmApiError> {
        info!(
            package_name = %package_name,
            version = %version,
            filename = %filename,
            "Processing npm package download request"
        );
        
        // Validar que el filename coincida con el formato esperado
        if !filename.ends_with(".tgz") {
            error!(
                filename = %filename,
                "Invalid filename format - must end with .tgz"
            );
            return Err(NpmApiError::BadRequest(
                "Invalid filename format - must end with .tgz".to_string()
            ));
        }
        
        // Parsear nombre del paquete
        let package_name_obj = NpmPackageName::new(&package_name)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid package name: {}", e)))?;
        
        // Parsear versión
        let version_obj = NpmVersion::new(&version)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid version: {}", e)))?;
        
        // Verificar que el filename coincida con el nombre y versión
        let expected_filename = format!("{}-{}.tgz", package_name_obj.package_name(), version);
        if !filename.starts_with(&expected_filename) {
            warn!(
                filename = %filename,
                expected_filename = %expected_filename,
                "Filename does not match expected format"
            );
        }
        
        // Crear request
        let request = NpmGetPackageRequest {
            package_name: package_name_obj,
            version: version_obj,
            repository_id,
        };
        
        // Ejecutar caso de uso
        let response = self.get_package_use_case.execute(request).await
            .map_err(NpmApiError::from)?;
        
        // Construir headers de respuesta
        let mut response_headers = HeaderMap::new();
        response_headers.insert("Content-Type", HeaderValue::from_static("application/octet-stream"));
        response_headers.insert("Content-Length", HeaderValue::from_str(&response.content_length.to_string()).unwrap());
        
        if let Some(ref etag) = response.etag {
            response_headers.insert("ETag", HeaderValue::from_str(etag).unwrap());
        }
        
        if let Some(last_modified) = response.last_modified {
            response_headers.insert("Last-Modified", HeaderValue::from_str(&last_modified.to_string()).unwrap());
        }
        
        if let Some(ref integrity) = response.integrity {
            response_headers.insert("X-Package-Integrity", HeaderValue::from_str(integrity).unwrap());
        }
        
        info!(
            package_name = %package_name,
            version = %version,
            content_length = response.content_length,
            "Successfully processed npm package download"
        );
        
        Ok(Response::builder()
            .status(StatusCode::OK)
            .headers(response_headers)
            .body(Body::from(response.content))
            .unwrap())
    }
    
    /// Manejar PUT request para publicar un paquete npm (.tgz)
    #[instrument(
        name = "npm.api.put_package",
        skip(self, headers, body),
        fields(
            package.name = %package_name,
            package.version = %version,
            filename = %filename
        )
    )]
    pub async fn handle_put_package(
        &self,
        Path((package_name, version, filename)): Path<(String, String, String)>,
        headers: HeaderMap,
        body: Body,
        Extension(repository_id): Extension<String>,
        Extension(user_id): Extension<String>,
    ) -> Result<Response<Body>, NpmApiError> {
        info!(
            package_name = %package_name,
            version = %version,
            filename = %filename,
            user_id = %user_id,
            "Processing npm package publish request"
        );
        
        // Validar que el filename coincida con el formato esperado
        if !filename.ends_with(".tgz") {
            error!(
                filename = %filename,
                "Invalid filename format - must end with .tgz"
            );
            return Err(NpmApiError::BadRequest(
                "Invalid filename format - must end with .tgz".to_string()
            ));
        }
        
        // Parsear nombre del paquete
        let package_name_obj = NpmPackageName::new(&package_name)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid package name: {}", e)))?;
        
        // Parsear versión
        let version_obj = NpmVersion::new(&version)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid version: {}", e)))?;
        
        // Leer el body completo
        let content = axum::body::to_bytes(body, 50 * 1024 * 1024) // Máximo 50MB
            .await
            .map_err(|e| NpmApiError::BadRequest(format!("Failed to read request body: {}", e)))?;
        
        // Determinar content-type
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        
        // Determinar si se permite sobrescribir
        let overwrite = headers
            .get("x-overwrite")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "true")
            .unwrap_or(false);
        
        // Crear request
        let request = NpmPutPackageRequest {
            package_name: package_name_obj,
            version: version_obj,
            content: content.to_vec(),
            content_type,
            repository_id,
            overwrite,
            metadata: None, // TODO: extraer metadata del tarball si es necesario
        };
        
        // Ejecutar caso de uso
        let response = self.put_package_use_case.execute(request).await
            .map_err(NpmApiError::from)?;
        
        // Construir respuesta JSON
        let response_json = serde_json::json!({
            "success": response.success,
            "message": response.message,
            "package": response.package_name,
            "version": response.version,
            "tarball_url": response.tarball_url,
            "size_bytes": response.size_bytes,
            "published_at": response.published_at
        });
        
        let response_body = serde_json::to_string(&response_json)
            .map_err(|e| NpmApiError::InternalServerError(format!("Failed to serialize response: {}", e)))?;
        
        info!(
            package_name = %package_name,
            version = %version,
            size_bytes = response.size_bytes,
            "Successfully published npm package"
        );
        
        Ok(Response::builder()
            .status(StatusCode::CREATED)
            .header("Content-Type", "application/json")
            .body(Body::from(response_body))
            .unwrap())
    }
    
    /// Manejar HEAD request para verificar existencia de un paquete
    #[instrument(
        name = "npm.api.head_package",
        skip(self),
        fields(
            package.name = %package_name,
            package.version = %version,
            filename = %filename
        )
    )]
    pub async fn handle_head_package(
        &self,
        Path((package_name, version, filename)): Path<(String, String, String)>,
        Extension(repository_id): Extension<String>,
    ) -> Result<Response<Body>, NpmApiError> {
        info!(
            package_name = %package_name,
            version = %version,
            filename = %filename,
            "Processing npm package head request"
        );
        
        // Validar que el filename coincida con el formato esperado
        if !filename.ends_with(".tgz") {
            error!(
                filename = %filename,
                "Invalid filename format - must end with .tgz"
            );
            return Err(NpmApiError::BadRequest(
                "Invalid filename format - must end with .tgz".to_string()
            ));
        }
        
        // Parsear nombre del paquete
        let package_name_obj = NpmPackageName::new(&package_name)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid package name: {}", e)))?;
        
        // Parsear versión
        let version_obj = NpmVersion::new(&version)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid version: {}", e)))?;
        
        // Crear request
        let request = NpmHeadPackageRequest {
            package_name: package_name_obj,
            version: version_obj,
            repository_id,
        };
        
        // Ejecutar caso de uso
        let response = self.head_package_use_case.execute(request).await
            .map_err(NpmApiError::from)?;
        
        // Construir headers de respuesta
        let mut response_headers = HeaderMap::new();
        
        if let Some(content_length) = response.content_length {
            response_headers.insert("Content-Length", HeaderValue::from_str(&content_length.to_string()).unwrap());
        }
        
        if let Some(ref etag) = response.etag {
            response_headers.insert("ETag", HeaderValue::from_str(etag).unwrap());
        }
        
        if let Some(last_modified) = response.last_modified {
            response_headers.insert("Last-Modified", HeaderValue::from_str(&last_modified.to_string()).unwrap());
        }
        
        let status = if response.exists {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        
        info!(
            package_name = %package_name,
            version = %version,
            exists = response.exists,
            "Completed npm package head check"
        );
        
        Ok(Response::builder()
            .status(status)
            .headers(response_headers)
            .body(Body::empty())
            .unwrap())
    }
    
    /// Manejar GET request para obtener metadata de un paquete
    #[instrument(
        name = "npm.api.get_package_json",
        skip(self),
        fields(
            package.name = %package_name,
            package.version = %version
        )
    )]
    pub async fn handle_get_package_json(
        &self,
        Path((package_name, version)): Path<(String, String)>,
        Extension(repository_id): Extension<String>,
    ) -> Result<Response<Body>, NpmApiError> {
        info!(
            package_name = %package_name,
            version = %version,
            "Processing npm package.json request"
        );
        
        // Parsear nombre del paquete
        let package_name_obj = NpmPackageName::new(&package_name)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid package name: {}", e)))?;
        
        // Parsear versión
        let version_obj = NpmVersion::new(&version)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid version: {}", e)))?;
        
        // Crear request
        let request = NpmGetPackageJsonRequest {
            package_name: package_name_obj,
            version: Some(version_obj),
            repository_id,
        };
        
        // Ejecutar caso de uso
        let response = self.get_package_json_use_case.execute(request).await
            .map_err(NpmApiError::from)?;
        
        // Construir respuesta JSON
        let response_body = serde_json::to_string(&response.package_json)
            .map_err(|e| NpmApiError::InternalServerError(format!("Failed to serialize response: {}", e)))?;
        
        info!(
            package_name = %package_name,
            version = %version,
            content_length = response.content_length,
            "Successfully retrieved npm package.json"
        );
        
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Body::from(response_body))
            .unwrap())
    }
    
    /// Manejar GET request para obtener metadata del repositorio
    #[instrument(
        name = "npm.api.get_repository_info",
        skip(self),
        fields(
            package.name = %package_name
        )
    )]
    pub async fn handle_get_repository_info(
        &self,
        Path(package_name): Path<String>,
        Extension(repository_id): Extension<String>,
    ) -> Result<Response<Body>, NpmApiError> {
        info!(
            package_name = %package_name,
            "Processing npm repository info request"
        );
        
        // Parsear nombre del paquete
        let package_name_obj = NpmPackageName::new(&package_name)
            .map_err(|e| NpmApiError::BadRequest(format!("Invalid package name: {}", e)))?;
        
        // Crear request
        let request = NpmGetRepositoryInfoRequest {
            package_name: package_name_obj,
            repository_id,
        };
        
        // Ejecutar caso de uso
        let response = self.get_package_json_use_case.execute(request).await
            .map_err(NpmApiError::from)?;
        
        // Construir respuesta JSON
        let response_body = serde_json::to_string(&response.package_json)
            .map_err(|e| NpmApiError::InternalServerError(format!("Failed to serialize response: {}", e)))?;
        
        info!(
            package_name = %package_name,
            versions_count = response.versions.len(),
            "Successfully retrieved npm repository info"
        );
        
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Body::from(response_body))
            .unwrap())
    }
}

/// Errores del API npm
#[derive(Debug, thiserror::Error)]
pub enum NpmApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Forbidden: {0}")]
    Forbidden(String),
    
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
}

impl From<super::ports::NpmReadError> for NpmApiError {
    fn from(error: super::ports::NpmReadError) -> Self {
        match error {
            super::ports::NpmReadError::PackageNotFound { .. } => {
                NpmApiError::NotFound(error.to_string())
            }
            super::ports::NpmReadError::RepositoryNotFound { .. } => {
                NpmApiError::RepositoryError(error.to_string())
            }
            super::ports::NpmReadError::PermissionDenied { .. } => {
                NpmApiError::Forbidden(error.to_string())
            }
            super::ports::NpmReadError::InvalidPackageName(_) |
            super::ports::NpmReadError::InvalidVersion(_) => {
                NpmApiError::BadRequest(error.to_string())
            }
            super::ports::NpmReadError::StorageError(_) => {
                NpmApiError::StorageError(error.to_string())
            }
            super::ports::NpmReadError::RepositoryError(_) => {
                NpmApiError::RepositoryError(error.to_string())
            }
            super::ports::NpmReadError::NetworkError(_) => {
                NpmApiError::InternalServerError(error.to_string())
            }
        }
    }
}

impl From<super::ports::NpmWriteError> for NpmApiError {
    fn from(error: super::ports::NpmWriteError) -> Self {
        match error {
            super::ports::NpmWriteError::PackageAlreadyExists { .. } => {
                NpmApiError::BadRequest(error.to_string())
            }
            super::ports::NpmWriteError::RepositoryNotFound { .. } => {
                NpmApiError::RepositoryError(error.to_string())
            }
            super::ports::NpmWriteError::PermissionDenied { .. } => {
                NpmApiError::Forbidden(error.to_string())
            }
            super::ports::NpmWriteError::InvalidPackageName(_) |
            super::ports::NpmWriteError::InvalidVersion(_) |
            super::ports::NpmWriteError::InvalidPackageContent(_) => {
                NpmApiError::BadRequest(error.to_string())
            }
            super::ports::NpmWriteError::StorageError(_) => {
                NpmApiError::StorageError(error.to_string())
            }
            super::ports::NpmWriteError::RepositoryError(_) => {
                NpmApiError::RepositoryError(error.to_string())
            }
            super::ports::NpmWriteError::NetworkError(_) => {
                NpmApiError::InternalServerError(error.to_string())
            }
            super::ports::NpmWriteError::PrivatePackage { .. } => {
                NpmApiError::BadRequest(error.to_string())
            }
        }
    }
}

impl IntoResponse for NpmApiError {
    fn into_response(self) -> Response<Body> {
        let (status, message) = match self {
            NpmApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            NpmApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            NpmApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            NpmApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            NpmApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NpmApiError::RepositoryError(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            NpmApiError::StorageError(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
        };
        
        let error_response = serde_json::json!({
            "error": {
                "message": message,
                "type": format!("{:?}", self)
            }
        });
        
        let body = serde_json::to_string(&error_response)
            .unwrap_or_else(|_| r#"{"error":{"message":"Internal server error","type":"InternalServerError"}}"#.to_string());
        
        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap()
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
    async fn test_handle_get_package_success() {
        let package_reader = Arc::new(MockNpmPackageReader::new());
        let repository_manager = Arc::new(MockNpmRepositoryManager::new());
        let permission_checker = Arc::new(MockNpmPermissionChecker::new());
        
        let get_use_case = Arc::new(HandleNpmGetPackageUseCase::new(
            package_reader.clone(),
            repository_manager,
            permission_checker,
        ));
        
        let put_use_case = Arc::new(HandleNpmPutPackageUseCase::new(
            Arc::new(MockNpmPackageWriter::new()),
            Arc::new(MockNpmRepositoryManager::new()),
            Arc::new(MockNpmPermissionChecker::new()),
        ));
        
        let head_use_case = Arc::new(HandleNpmHeadPackageUseCase::new(
            package_reader.clone(),
            Arc::new(MockNpmRepositoryManager::new()),
            Arc::new(MockNpmPermissionChecker::new()),
        ));
        
        let get_json_use_case = Arc::new(HandleNpmGetPackageJsonUseCase::new(
            package_reader,
            Arc::new(MockNpmRepositoryManager::new()),
            Arc::new(MockNpmPermissionChecker::new()),
        ));
        
        let handler = NpmRequestHandler::new(
            get_use_case,
            put_use_case,
            head_use_case,
            get_json_use_case,
        );
        
        // Agregar un paquete mock
        let name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        let key = format!("{}@{}", name.full_name(), version);
        handler.get_package_use_case.package_reader.add_package(key, b"test content".to_vec());
        
        let result = handler.handle_get_package(
            Path(("test-package".to_string(), "1.0.0".to_string(), "test-package-1.0.0.tgz".to_string())),
            HeaderMap::new(),
            Extension("npm-repo".to_string()),
        ).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}