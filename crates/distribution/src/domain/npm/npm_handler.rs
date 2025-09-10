// crates/distribution/src/domain/npm/npm_handler.rs

use async_trait::async_trait;
use shared::enums::Ecosystem;
use shared::hrn::{RepositoryId, UserId};
use crate::domain::format_handler::{FormatHandler, FormatRequest, FormatResponse, PackageMetadata};
use crate::domain::error::{DistributionError, DistributionResult};
use super::{NpmPathParser, NpmPathInfo, NpmMetadataGenerator, NpmPackageMetadata, NpmPackageVersion};
use std::collections::HashMap;

/// Manejador de formato npm
pub struct NpmFormatHandler {
    path_parser: NpmPathParser,
    metadata_generator: NpmMetadataGenerator,
}

impl NpmFormatHandler {
    pub fn new() -> Self {
        Self {
            path_parser: NpmPathParser::new(),
            metadata_generator: NpmMetadataGenerator::new(),
        }
    }

    /// Procesa una publicación de paquete npm
    async fn handle_publish(
        &self,
        request: &FormatRequest,
        path_info: &NpmPathInfo,
    ) -> DistributionResult<FormatResponse> {
        // Validar que tenemos body (package.json)
        let body = request.body.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing package.json body".to_string()))?;

        // Parsear el package.json
        let package_json = String::from_utf8(body.clone())
            .map_err(|e| DistributionError::InvalidRequest(format!("Invalid UTF-8 in package.json: {}", e)))?;

        let version_metadata = self.metadata_generator.parse_package_json(&package_json)?;

        tracing::info!(
            "Processing npm publish: {}@{}",
            version_metadata.name,
            version_metadata.version
        );

        // Aquí iría la lógica real de almacenamiento
        // Por ahora, simulamos éxito
        Ok(FormatResponse {
            status_code: 201,
            headers: self.get_standard_headers(),
            body: None,
            content_type: None,
        })
    }

    /// Procesa una descarga de tarball npm
    async fn handle_download(
        &self,
        _request: &FormatRequest,
        path_info: &NpmPathInfo,
    ) -> DistributionResult<FormatResponse> {
        let package_name = &path_info.package_name;
        let version = path_info.version.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing version for tarball download".to_string()))?;

        tracing::info!(
            "Processing npm download: {}@{}",
            package_name,
            version
        );

        // Aquí iría la lógica real de recuperación del tarball
        // Por ahora, simulamos con datos de prueba
        let mock_content = format!(
            "Mock npm tarball: {}@{}",
            package_name,
            version
        );

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Length".to_string(), mock_content.len().to_string());
        headers.insert("Content-Type".to_string(), "application/octet-stream".to_string());
        headers.insert("Content-Disposition".to_string(), format!("attachment; filename=\"{}-{}.tgz\"", package_name, version));

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(mock_content.into_bytes()),
            content_type: Some("application/octet-stream".to_string()),
        })
    }

    /// Genera metadata del paquete npm
    async fn generate_package_metadata_response(
        &self,
        package_name: &str,
        version: Option<&str>,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Generating npm metadata for {} {}",
            package_name,
            version.map(|v| format!("@{}", v)).unwrap_or_else(|| "all versions".to_string())
        );

        // En producción, esto consultaría la base de datos
        // Por ahora, usamos datos de prueba
        let versions = vec![
            ("1.0.0".to_string(), self.create_mock_version(package_name, "1.0.0")),
            ("1.1.0".to_string(), self.create_mock_version(package_name, "1.1.0")),
            ("2.0.0".to_string(), self.create_mock_version(package_name, "2.0.0")),
        ];

        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "2.0.0".to_string());

        let metadata = if let Some(v) = version {
            // Metadata específica de versión
            if let Some((_, version_metadata)) = versions.iter().find(|(ver, _)| ver == v) {
                let json = serde_json::to_string_pretty(version_metadata)
                    .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?;
                
                let mut headers = self.get_standard_headers();
                headers.insert("Content-Type".to_string(), "application/json".to_string());

                return Ok(FormatResponse {
                    status_code: 200,
                    headers,
                    body: Some(json.into_bytes()),
                    content_type: Some("application/json".to_string()),
                });
            } else {
                return Err(DistributionError::ArtifactNotFound(format!("Version {} not found for package {}", v, package_name)));
            }
        } else {
            // Metadata completa del paquete
            self.metadata_generator.generate_package_metadata(package_name, versions, dist_tags)?
        };

        let json = self.metadata_generator.metadata_to_json(&metadata)?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }

    /// Genera dist-tags del paquete
    async fn generate_dist_tags_response(
        &self,
        package_name: &str,
        tag: Option<&str>,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Generating npm dist-tags for {} {}",
            package_name,
            tag.map(|t| format!("tag: {}", t)).unwrap_or_else(|| "all tags".to_string())
        );

        // En producción, esto consultaría la base de datos
        // Por ahora, usamos datos de prueba
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "2.0.0".to_string());
        dist_tags.insert("beta".to_string(), "2.1.0-beta.1".to_string());
        dist_tags.insert("alpha".to_string(), "3.0.0-alpha.1".to_string());

        let json = if let Some(t) = tag {
            // Tag específico
            if let Some(version) = dist_tags.get(t) {
                serde_json::to_string(version)
                    .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?
            } else {
                return Err(DistributionError::ArtifactNotFound(format!("Tag {} not found for package {}", t, package_name)));
            }
        } else {
            // Todos los tags
            serde_json::to_string_pretty(&dist_tags)
                .map_err(|e| DistributionError::MetadataGenerationFailed(e.to_string()))?
        };

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(json.into_bytes()),
            content_type: Some("application/json".to_string()),
        })
    }

    /// Obtiene headers estándar para respuestas npm
    fn get_standard_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Server".to_string(), "Hodei-Artifacts-npm".to_string());
        headers.insert("Cache-Control".to_string(), "public, max-age=300".to_string()); // 5 minutos para npm
        headers.insert("Accept-Ranges".to_string(), "bytes".to_string());
        headers
    }

    /// Crea una versión mock para pruebas
    fn create_mock_version(&self, package_name: &str, version: &str) -> NpmPackageVersion {
        NpmPackageVersion {
            name: package_name.to_string(),
            version: version.to_string(),
            description: Some(format!("{} version {}", package_name, version)),
            main: Some("index.js".to_string()),
            scripts: HashMap::new(),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            peer_dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            bundled_dependencies: vec![],
            keywords: vec!["npm".to_string(), "package".to_string()],
            author: Some("System <system@example.com>".to_string()),
            license: Some("MIT".to_string()),
            repository: Some(super::npm_metadata::RepositoryInfo {
                repo_type: "git".to_string(),
                url: format!("https://github.com/user/{}.git", package_name),
                directory: None,
            }),
            bugs: Some(super::npm_metadata::BugsInfo {
                url: Some(format!("https://github.com/user/{}/issues", package_name)),
                email: None,
            }),
            homepage: Some(format!("https://github.com/user/{}", package_name)),
            engines: HashMap::new(),
            os: vec![],
            cpu: vec![],
            dist: super::npm_metadata::DistInfo {
                integrity: format!("sha512-{}", version),
                shasum: format!("shasum{}", version),
                tarball: format!("https://registry.npmjs.org/{}/-/{}-{}.tgz", package_name, package_name, version),
                file_count: Some(10),
                unpacked_size: Some(1024),
                npm_signature: None,
            },
            directories: None,
            files: vec!["index.js".to_string(), "package.json".to_string()],
            publish_config: None,
            _has_shrinkwrap: Some(false),
        }
    }

    /// Lista versiones disponibles para un paquete npm
    async fn list_npm_versions(
        &self,
        _repository_hrn: &RepositoryId,
        package_name: &str,
        _user_id: &UserId,
    ) -> DistributionResult<Vec<String>> {
        tracing::info!("Listing npm versions for {}", package_name);

        // En producción, esto consultaría la base de datos
        // Por ahora, datos de prueba
        Ok(vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "2.0.0".to_string(),
            "2.1.0-beta.1".to_string(),
            "3.0.0-alpha.1".to_string(),
        ])
    }
}

#[async_trait]
impl FormatHandler for NpmFormatHandler {
    async fn handle_request(&self, request: FormatRequest) -> DistributionResult<FormatResponse> {
        // Parsear el path
        let path_info = self.path_parser.parse_path(&request.path)?;
        
        tracing::info!(
            "npm handler processing {} request for path: {}",
            request.method,
            request.path
        );

        match request.method.as_str() {
            "GET" => {
                if path_info.is_metadata {
                    if path_info.package_name.starts_with("-/package/") && path_info.path.contains("/dist-tags") {
                        // Dist-tags request
                        let package_name = path_info.package_name.trim_start_matches("-/package/");
                        let tag = if path_info.path.ends_with("/dist-tags") {
                            None
                        } else {
                            path_info.path.split('/').last()
                        };
                        self.generate_dist_tags_response(package_name, tag).await
                    } else {
                        // Regular package metadata
                        self.generate_package_metadata_response(&path_info.package_name, path_info.version.as_deref()).await
                    }
                } else if path_info.is_tarball {
                    self.handle_download(&request, &path_info).await
                } else {
                    Err(DistributionError::InvalidRequest("Unsupported npm GET request".to_string()))
                }
            }
            "PUT" => {
                if path_info.path.contains("-/user/") && path_info.path.contains("/package/") {
                    // Publish request
                    self.handle_publish(&request, &path_info).await
                } else {
                    Err(DistributionError::InvalidRequest("Unsupported npm PUT request".to_string()))
                }
            }
            _ => {
                Err(DistributionError::InvalidRequest(
                    format!("Unsupported HTTP method for npm: {}", request.method)
                ))
            }
        }
    }

    fn can_handle(&self, path: &str, ecosystem: &Ecosystem) -> bool {
        *ecosystem == Ecosystem::Npm && !path.is_empty()
    }

    fn supported_ecosystem(&self) -> Ecosystem {
        Ecosystem::Npm
    }

    async fn generate_repository_metadata(
        &self,
        repository_hrn: &RepositoryId,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Generating npm repository metadata for {}",
            repository_hrn.as_str()
        );

        // En producción, esto agregaría todos los paquetes del repositorio
        // Por ahora, simulamos con datos de prueba
        let mock_metadata = serde_json::json!({
            "format": "npm",
            "repository": repository_hrn.as_str(),
            "packages": [
                {
                    "name": "my-package",
                    "versions": ["1.0.0", "1.1.0"]
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
        self.list_npm_versions(repository_hrn, package_name, user_id).await
    }

    async fn get_version_info(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        version: &str,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        // Crear metadata del paquete
        let version_metadata = self.create_mock_version(package_name, version);

        let json = serde_json::to_string_pretty(&version_metadata)
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
    async fn test_can_handle_npm() {
        let handler = NpmFormatHandler::new();
        
        assert!(handler.can_handle("/my-package", &Ecosystem::Npm));
        assert!(handler.can_handle("/my-package/1.0.0", &Ecosystem::Npm));
        assert!(handler.can_handle("/my-package-/my-package-1.0.0.tgz", &Ecosystem::Npm));
        assert!(!handler.can_handle("/com/example/my-app/1.0.0/my-app-1.0.0.jar", &Ecosystem::Npm));
        assert!(!handler.can_handle("/my-package", &Ecosystem::Maven));
    }

    #[tokio::test]
    async fn test_supported_ecosystem() {
        let handler = NpmFormatHandler::new();
        assert_eq!(handler.supported_ecosystem(), Ecosystem::Npm);
    }

    #[tokio::test]
    async fn test_handle_metadata_request() {
        let handler = NpmFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Npm,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "npm-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/my-package".to_string(),
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
        assert!(json.contains("\"name\": \"my-package\""));
        assert!(json.contains("\"versions\""));
    }

    #[tokio::test]
    async fn test_handle_version_metadata_request() {
        let handler = NpmFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Npm,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "npm-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/my-package/1.0.0".to_string(),
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
        assert!(json.contains("\"name\": \"my-package\""));
        assert!(json.contains("\"version\": \"1.0.0\""));
    }

    #[tokio::test]
    async fn test_handle_tarball_request() {
        let handler = NpmFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Npm,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "npm-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/my-package-/my-package-1.0.0.tgz".to_string(),
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
    async fn test_handle_dist_tags_request() {
        let handler = NpmFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Npm,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "npm-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/-/package/my-package/dist-tags".to_string(),
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
        assert!(json.contains("\"latest\""));
        assert!(json.contains("\"beta\""));
        assert!(json.contains("\"alpha\""));
    }
}