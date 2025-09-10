// crates/distribution/src/domain/docker/docker_handler.rs

use async_trait::async_trait;
use shared::enums::Ecosystem;
use shared::hrn::{RepositoryId, UserId};
use crate::domain::format_handler::{FormatHandler, FormatRequest, FormatResponse, PackageMetadata};
use crate::domain::error::{DistributionError, DistributionResult};
use super::{DockerPathParser, DockerPathInfo, DockerManifestGenerator, DockerManifestV2, DockerManifestList};
use std::collections::HashMap;

/// Manejador de formato Docker Registry V2
pub struct DockerFormatHandler {
    path_parser: DockerPathParser,
    manifest_generator: DockerManifestGenerator,
}

impl DockerFormatHandler {
    pub fn new() -> Self {
        Self {
            path_parser: DockerPathParser::new(),
            manifest_generator: DockerManifestGenerator::new(),
        }
    }

    /// Procesa un push de imagen Docker
    async fn handle_push(
        &self,
        request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Processing Docker push: {}/{}",
            path_info.namespace,
            path_info.repository
        );

        // Determinar el tipo de operación según el path
        if path_info.is_manifest {
            self.handle_push_manifest(request, path_info).await
        } else if path_info.is_blob {
            self.handle_push_blob(request, path_info).await
        } else {
            Err(DistributionError::InvalidRequest("Unsupported Docker push operation".to_string()))
        }
    }

    /// Procesa un pull de imagen Docker
    async fn handle_pull(
        &self,
        request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Processing Docker pull: {}/{}",
            path_info.namespace,
            path_info.repository
        );

        // Determinar el tipo de operación según el path
        if path_info.is_manifest {
            self.handle_pull_manifest(request, path_info).await
        } else if path_info.is_blob {
            self.handle_pull_blob(request, path_info).await
        } else if path_info.is_catalog {
            self.handle_catalog(request).await
        } else if path_info.is_tags {
            self.handle_tags_list(request, path_info).await
        } else {
            Err(DistributionError::InvalidRequest("Unsupported Docker pull operation".to_string()))
        }
    }

    /// Maneja el push de un manifest
    async fn handle_push_manifest(
        &self,
        request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        let body = request.body.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing manifest body".to_string()))?;

        let manifest_json = String::from_utf8(body.clone())
            .map_err(|e| DistributionError::InvalidRequest(format!("Invalid UTF-8 in manifest: {}", e)))?;

        // Determinar el tipo de manifest según el Content-Type
        let content_type = request.headers.get("content-type")
            .ok_or_else(|| DistributionError::InvalidRequest("Missing Content-Type header".to_string()))?;

        let manifest = match content_type.as_str() {
            "application/vnd.docker.distribution.manifest.v2+json" => {
                self.manifest_generator.parse_manifest_v2(&manifest_json)?
            }
            "application/vnd.docker.distribution.manifest.list.v2+json" => {
                // Parsear como manifest list
                return Err(DistributionError::NotImplemented("Manifest list parsing not implemented".to_string()));
            }
            _ => {
                return Err(DistributionError::InvalidRequest(format!("Unsupported manifest media type: {}", content_type)));
            }
        };

        // Generar digest del manifest
        let digest = self.manifest_generator.generate_manifest_digest(&manifest_json);

        tracing::info!(
            "Pushing Docker manifest: {}/{} with digest: {}",
            path_info.namespace,
            path_info.repository,
            digest
        );

        // Aquí iría la lógica real de almacenamiento
        // Por ahora, simulamos éxito

        let mut headers = self.get_standard_headers();
        headers.insert("Docker-Content-Digest".to_string(), digest);
        headers.insert("Location".to_string(), format!("/v2/{}/{}/manifests/{}", 
            path_info.namespace, path_info.repository, digest));

        Ok(FormatResponse {
            status_code: 201,
            headers,
            body: None,
            content_type: None,
        })
    }

    /// Maneja el push de un blob
    async fn handle_push_blob(
        &self,
        request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        let body = request.body.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing blob body".to_string()))?;

        // Generar digest del blob
        let digest = self.manifest_generator.generate_blob_digest(body);

        tracing::info!(
            "Pushing Docker blob: {}/{} with digest: {}",
            path_info.namespace,
            path_info.repository,
            digest
        );

        // Aquí iría la lógica real de almacenamiento
        // Por ahora, simulamos éxito

        let mut headers = self.get_standard_headers();
        headers.insert("Docker-Content-Digest".to_string(), digest.clone());
        headers.insert("Location".to_string(), format!("/v2/{}/{}/blobs/{}", 
            path_info.namespace, path_info.repository, digest));

        Ok(FormatResponse {
            status_code: 201,
            headers,
            body: None,
            content_type: None,
        })
    }

    /// Maneja el pull de un manifest
    async fn handle_pull_manifest(
        &self,
        _request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        let reference = path_info.reference.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing manifest reference".to_string()))?;

        tracing::info!(
            "Pulling Docker manifest: {}/{}@{}",
            path_info.namespace,
            path_info.repository,
            reference
        );

        // En producción, esto consultaría la base de datos
        // Por ahora, generamos un manifest mock

        let config_digest = "sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let config_size = 1234;
        let layers = vec![
            ("sha256:layer1digest1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab".to_string(), 5678),
            ("sha256:layer2digest1234567890abcdef1234567890abcdef1234567890abcdef1234567890cd".to_string(), 9012),
        ];

        let manifest = self.manifest_generator.generate_manifest_v2(config_digest, config_size, layers)?;
        let manifest_json = self.manifest_generator.serialize_manifest_v2(&manifest)?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/vnd.docker.distribution.manifest.v2+json".to_string());
        headers.insert("Docker-Content-Digest".to_string(), self.manifest_generator.generate_manifest_digest(&manifest_json));
        headers.insert("Content-Length".to_string(), manifest_json.len().to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(manifest_json.into_bytes()),
            content_type: Some("application/vnd.docker.distribution.manifest.v2+json".to_string()),
        })
    }

    /// Maneja el pull de un blob
    async fn handle_pull_blob(
        &self,
        _request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        let digest = path_info.digest.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing blob digest".to_string()))?;

        tracing::info!(
            "Pulling Docker blob: {}/{}@{}",
            path_info.namespace,
            path_info.repository,
            digest
        );

        // En producción, esto consultaría el almacenamiento
        // Por ahora, simulamos con datos de prueba
        let mock_content = format!(
            "Mock Docker blob content for {}/{}@{}",
            path_info.namespace,
            path_info.repository,
            digest
        );

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/octet-stream".to_string());
        headers.insert("Content-Length".to_string(), mock_content.len().to_string());
        headers.insert("Docker-Content-Digest".to_string(), digest.to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(mock_content.into_bytes()),
            content_type: Some("application/octet-stream".to_string()),
        })
    }

    /// Maneja el listado del catalogo
    async fn handle_catalog(&self, _request: &FormatRequest) -> DistributionResult<FormatResponse> {
        tracing::info!("Listing Docker catalog");

        // En producción, esto consultaría la base de datos
        // Por ahora, datos de prueba
        let catalog = serde_json::json!({
            "repositories": [
                "library/nginx",
                "library/redis",
                "library/postgres",
                "custom/app"
            ]
        });

        let catalog_json = serde_json::to_string_pretty(&catalog)
            .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(catalog_json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }

    /// Maneja el listado de tags
    async fn handle_tags_list(
        &self,
        _request: &FormatRequest,
        path_info: &DockerPathInfo,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Listing Docker tags for: {}/{}",
            path_info.namespace,
            path_info.repository
        );

        // En producción, esto consultaría la base de datos
        // Por ahora, datos de prueba
        let tags = serde_json::json!({
            "name": format!("{}/{}", path_info.namespace, path_info.repository),
            "tags": [
                "latest",
                "1.0.0",
                "1.1.0",
                "2.0.0"
            ]
        });

        let tags_json = serde_json::to_string_pretty(&tags)
            .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(tags_json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }

    /// Obtiene headers estándar para respuestas Docker
    fn get_standard_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Server".to_string(), "Hodei-Artifacts-docker".to_string());
        headers.insert("Cache-Control".to_string(), "public, max-age=300".to_string()); // 5 minutos
        headers.insert("Accept-Ranges".to_string(), "bytes".to_string());
        headers
    }

    /// Lista versiones disponibles para una imagen Docker
    async fn list_docker_tags(
        &self,
        _repository_hrn: &RepositoryId,
        namespace: &str,
        repository: &str,
        _user_id: &UserId,
    ) -> DistributionResult<Vec<String>> {
        tracing::info!("Listing Docker tags for {}/{}", namespace, repository);

        // En producción, esto consultaría la base de datos
        // Por ahora, datos de prueba
        Ok(vec![
            "latest".to_string(),
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "2.0.0".to_string(),
        ])
    }
}

#[async_trait]
impl FormatHandler for DockerFormatHandler {
    async fn handle_request(&self, request: FormatRequest) -> DistributionResult<FormatResponse> {
        // Parsear el path
        let path_info = self.path_parser.parse_path(&request.path)?;
        
        tracing::info!(
            "Docker handler processing {} request for path: {}",
            request.method,
            request.path
        );

        match request.method.as_str() {
            "GET" => {
                self.handle_pull(&request, &path_info).await
            }
            "PUT" => {
                self.handle_push(&request, &path_info).await
            }
            "HEAD" => {
                // HEAD requests para verificar existencia sin descargar
                self.handle_pull(&request, &path_info).await
            }
            _ => {
                Err(DistributionError::InvalidRequest(
                    format!("Unsupported HTTP method for Docker: {}", request.method)
                ))
            }
        }
    }

    fn can_handle(&self, path: &str, ecosystem: &Ecosystem) -> bool {
        *ecosystem == Ecosystem::Docker && path.starts_with("/v2/")
    }

    fn supported_ecosystem(&self) -> Ecosystem {
        Ecosystem::Docker
    }

    async fn generate_repository_metadata(
        &self,
        repository_hrn: &RepositoryId,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Generating Docker repository metadata for {}",
            repository_hrn.as_str()
        );

        // En producción, esto agregaría todas las imágenes del repositorio
        // Por ahora, simulamos con datos de prueba
        let mock_metadata = serde_json::json!({
            "format": "docker",
            "repository": repository_hrn.as_str(),
            "images": [
                {
                    "namespace": "library",
                    "repository": "nginx",
                    "tags": ["latest", "1.21", "1.22"]
                }
            ]
        });

        let json = serde_json::to_string_pretty(&mock_metadata)
            .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }

    async fn list_versions(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        user_id: &UserId,
    ) -> DistributionResult<Vec<String>> {
        // Para Docker, package_name tiene formato "namespace/repository"
        let parts: Vec<&str> = package_name.split('/').collect();
        if parts.len() != 2 {
            return Err(DistributionError::InvalidPackageFormat(
                format!("Invalid Docker image name format: {}. Expected namespace/repository", package_name)
            ));
        }
        
        let (namespace, repository) = (parts[0], parts[1]);
        self.list_docker_tags(repository_hrn, namespace, repository, user_id).await
    }

    async fn get_version_info(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        version: &str,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        // Para Docker, generar información de la imagen
        let parts: Vec<&str> = package_name.split('/').collect();
        if parts.len() != 2 {
            return Err(DistributionError::InvalidPackageFormat(
                format!("Invalid Docker image name format: {}. Expected namespace/repository", package_name)
            ));
        }
        
        let (namespace, repository) = (parts[0], parts[1]);

        // Crear metadata de la imagen
        let image_info = serde_json::json!({
            "namespace": namespace,
            "repository": repository,
            "tag": version,
            "digest": format!("sha256:{}", version),
            "size": 1024000,
            "architecture": "amd64",
            "os": "linux",
            "created": "2024-01-01T00:00:00Z"
        });

        let json = serde_json::to_string_pretty(&image_info)
            .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_can_handle_docker() {
        let handler = DockerFormatHandler::new();
        
        assert!(handler.can_handle("/v2/library/nginx/manifests/latest", &Ecosystem::Docker));
        assert!(handler.can_handle("/v2/library/nginx/blobs/sha256:123", &Ecosystem::Docker));
        assert!(handler.can_handle("/v2/_catalog", &Ecosystem::Docker));
        assert!(!handler.can_handle("/my-package", &Ecosystem::Docker));
        assert!(!handler.can_handle("/v2/library/nginx/manifests/latest", &Ecosystem::Maven));
    }

    #[tokio::test]
    async fn test_supported_ecosystem() {
        let handler = DockerFormatHandler::new();
        assert_eq!(handler.supported_ecosystem(), Ecosystem::Docker);
    }

    #[tokio::test]
    async fn test_handle_manifest_pull() {
        let handler = DockerFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Docker,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "docker-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/v2/library/nginx/manifests/latest".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/vnd.docker.distribution.manifest.v2+json".to_string()));
        assert!(response.body.is_some());
        
        let json = String::from_utf8(response.body.unwrap()).unwrap();
        assert!(json.contains("\"schemaVersion\": 2"));
        assert!(json.contains("\"mediaType\": \"application/vnd.docker.distribution.manifest.v2+json\""));
    }

    #[tokio::test]
    async fn test_handle_blob_pull() {
        let handler = DockerFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Docker,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "docker-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/v2/library/nginx/blobs/sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/octet-stream".to_string()));
        assert!(response.body.is_some());
    }

    #[tokio::test]
    async fn test_handle_catalog() {
        let handler = DockerFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Docker,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "docker-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/v2/_catalog".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/json".to_string()));
        assert!(response.body.is_some());
        
        let json = String::from_utf8(response.body.unwrap()).unwrap();
        assert!(json.contains("\"repositories\""));
        assert!(json.contains("\"library/nginx\""));
    }

    #[tokio::test]
    async fn test_handle_tags_list() {
        let handler = DockerFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Docker,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "docker-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/v2/library/nginx/tags/list".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/json".to_string()));
        assert!(response.body.is_some());
        
        let json = String::from_utf8(response.body.unwrap()).unwrap();
        assert!(json.contains("\"name\": \"library/nginx\""));
        assert!(json.contains("\"tags\""));
        assert!(json.contains("\"latest\""));
    }
}