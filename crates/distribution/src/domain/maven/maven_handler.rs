// crates/distribution/src/domain/maven/maven_handler.rs

use async_trait::async_trait;
use shared::enums::Ecosystem;
use shared::hrn::{RepositoryId, UserId};
use shared::models::PackageCoordinates;
use crate::domain::format_handler::{FormatHandler, FormatRequest, FormatResponse, PackageMetadata};
use crate::domain::error::{DistributionError, DistributionResult};
use super::{MavenPathParser, MavenPathInfo, MavenMetadataGenerator, MavenMetadata};
use std::collections::HashMap;

/// Manejador de formato Maven
pub struct MavenFormatHandler {
    path_parser: MavenPathParser,
    metadata_generator: MavenMetadataGenerator,
}

impl MavenFormatHandler {
    pub fn new() -> Self {
        Self {
            path_parser: MavenPathParser::new(),
            metadata_generator: MavenMetadataGenerator::new(),
        }
    }

    /// Procesa una subida de artefacto Maven
    async fn handle_upload(
        &self,
        request: &FormatRequest,
        path_info: &MavenPathInfo,
    ) -> DistributionResult<FormatResponse> {
        // Validar que tenemos body
        let body = request.body.as_ref()
            .ok_or_else(|| DistributionError::InvalidRequest("Missing artifact body".to_string()))?;

        // Validar tipo de artefacto
        match path_info.extension.as_str() {
            "jar" | "pom" | "war" | "ear" => {
                // Tipos válidos de artefactos Maven
            }
            _ => {
                return Err(DistributionError::InvalidPackageFormat(
                    format!("Unsupported Maven artifact type: {}", path_info.extension)
                ));
            }
        }

        // Aquí iría la lógica real de almacenamiento
        // Por ahora, simulamos éxito
        tracing::info!(
            "Processing Maven upload: {}:{}:{}",
            path_info.group_id,
            path_info.artifact_id,
            path_info.version
        );

        Ok(FormatResponse {
            status_code: 201,
            headers: self.get_standard_headers(),
            body: None,
            content_type: None,
        })
    }

    /// Procesa una descarga de artefacto Maven
    async fn handle_download(
        &self,
        _request: &FormatRequest,
        path_info: &MavenPathInfo,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Processing Maven download: {}:{}:{}",
            path_info.group_id,
            path_info.artifact_id,
            path_info.version
        );

        // Aquí iría la lógica real de recuperación del artefacto
        // Por ahora, simulamos con datos de prueba
        let mock_content = format!(
            "Mock Maven artifact: {}:{}:{}",
            path_info.group_id,
            path_info.artifact_id,
            path_info.version
        );

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Length".to_string(), mock_content.len().to_string());
        headers.insert("Content-Type".to_string(), self.get_content_type(&path_info.extension));

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(mock_content.into_bytes()),
            content_type: Some(self.get_content_type(&path_info.extension)),
        })
    }

    /// Genera maven-metadata.xml dinámicamente
    async fn generate_metadata_response(
        &self,
        path_info: &MavenPathInfo,
    ) -> DistributionResult<FormatResponse> {
        // En producción, esto consultaría la base de datos
        // Por ahora, usamos datos de prueba
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "1.2.0".to_string(),
        ];

        let xml = self.metadata_generator.generate_metadata(
            &path_info.group_id,
            &path_info.artifact_id,
            versions,
        )?;

        let mut headers = self.get_standard_headers();
        headers.insert("Content-Type".to_string(), "application/xml".to_string());
        headers.insert("Content-Length".to_string(), xml.len().to_string());

        Ok(FormatResponse {
            status_code: 200,
            headers,
            body: Some(xml.into_bytes()),
            content_type: Some("application/xml".to_string()),
        })
    }

    /// Obtiene headers estándar para respuestas Maven
    fn get_standard_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Server".to_string(), "Hodei-Artifacts-Maven".to_string());
        headers.insert("Cache-Control".to_string(), "public, max-age=3600".to_string());
        headers.insert("Accept-Ranges".to_string(), "bytes".to_string());
        headers
    }

    /// Determina el content-type basado en la extensión
    fn get_content_type(&self, extension: &str) -> String {
        match extension {
            "jar" => "application/java-archive",
            "pom" => "application/xml",
            "war" => "application/java-archive",
            "ear" => "application/java-archive",
            "xml" => "application/xml",
            _ => "application/octet-stream",
        }.to_string()
    }

    /// Lista versiones disponibles para un artefacto Maven
    async fn list_maven_versions(
        &self,
        _repository_hrn: &RepositoryId,
        group_id: &str,
        artifact_id: &str,
        _user_id: &UserId,
    ) -> DistributionResult<Vec<String>> {
        tracing::info!(
            "Listing Maven versions for {}:{}",
            group_id,
            artifact_id
        );

        // En producción, esto consultaría la base de datos
        // Por ahora, datos de prueba
        Ok(vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "1.2.0".to_string(),
            "2.0.0-SNAPSHOT".to_string(),
        ])
    }
}

#[async_trait]
impl FormatHandler for MavenFormatHandler {
    async fn handle_request(&self, request: FormatRequest) -> DistributionResult<FormatResponse> {
        // Parsear el path
        let path_info = self.path_parser.parse_path(&request.path)?;
        
        tracing::info!(
            "Maven handler processing {} request for path: {}",
            request.method,
            request.path
        );

        match request.method.as_str() {
            "GET" => {
                if path_info.is_metadata {
                    self.generate_metadata_response(&path_info).await
                } else {
                    self.handle_download(&request, &path_info).await
                }
            }
            "PUT" => {
                self.handle_upload(&request, &path_info).await
            }
            _ => {
                Err(DistributionError::InvalidRequest(
                    format!("Unsupported HTTP method for Maven: {}", request.method)
                ))
            }
        }
    }

    fn can_handle(&self, path: &str, ecosystem: &Ecosystem) -> bool {
        *ecosystem == Ecosystem::Maven && !path.is_empty()
    }

    fn supported_ecosystem(&self) -> Ecosystem {
        Ecosystem::Maven
    }

    async fn generate_repository_metadata(
        &self,
        repository_hrn: &RepositoryId,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        tracing::info!(
            "Generating Maven repository metadata for {}",
            repository_hrn.as_str()
        );

        // En producción, esto agregaría todos los artefactos del repositorio
        // Por ahora, simulamos con datos de prueba
        let mock_metadata = serde_json::json!({
            "format": "maven",
            "repository": repository_hrn.as_str(),
            "artifacts": [
                {
                    "groupId": "com.example",
                    "artifactId": "my-app",
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
        // Parsear el nombre del paquete para extraer groupId y artifactId
        let parts: Vec<&str> = package_name.split(':').collect();
        if parts.len() != 2 {
            return Err(DistributionError::InvalidPackageFormat(
                format!("Invalid Maven package name format: {}. Expected groupId:artifactId", package_name)
            ));
        }

        let (group_id, artifact_id) = (parts[0], parts[1]);
        self.list_maven_versions(repository_hrn, group_id, artifact_id, user_id).await
    }

    async fn get_version_info(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        version: &str,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse> {
        // Parsear el nombre del paquete
        let parts: Vec<&str> = package_name.split(':').collect();
        if parts.len() != 2 {
            return Err(DistributionError::InvalidPackageFormat(
                format!("Invalid Maven package name format: {}", package_name)
            ));
        }

        let (group_id, artifact_id) = (parts[0], parts[1]);

        // Crear metadata del paquete
        let metadata = PackageMetadata {
            name: format!("{}:{}", group_id, artifact_id),
            version: version.to_string(),
            description: Some(format!("Maven artifact {}:{}:{}", group_id, artifact_id, version)),
            licenses: vec!["Apache-2.0".to_string()], // Por defecto
            dependencies: vec![], // En producción, esto vendría del POM
            published_at: Some("2024-01-01T00:00:00Z".to_string()),
            download_url: Some(format!("/maven-repo/{}/{}/{}/{}-{}.jar", 
                group_id.replace('.', '/'), artifact_id, version, artifact_id, version)),
        };

        let json = serde_json::to_string_pretty(&metadata)
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
    async fn test_can_handle_maven() {
        let handler = MavenFormatHandler::new();
        
        assert!(handler.can_handle("/com/example/my-app/1.0.0/my-app-1.0.0.jar", &Ecosystem::Maven));
        assert!(handler.can_handle("/com/example/my-app/maven-metadata.xml", &Ecosystem::Maven));
        assert!(!handler.can_handle("/npm/package/-/package-1.0.0.tgz", &Ecosystem::Maven));
        assert!(!handler.can_handle("/com/example/my-app/1.0.0/my-app-1.0.0.jar", &Ecosystem::Npm));
    }

    #[tokio::test]
    async fn test_supported_ecosystem() {
        let handler = MavenFormatHandler::new();
        assert_eq!(handler.supported_ecosystem(), Ecosystem::Maven);
    }

    #[tokio::test]
    async fn test_handle_download_request() {
        let handler = MavenFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Maven,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "maven-repo"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/com/example/my-app/1.0.0/my-app-1.0.0.jar".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/java-archive".to_string()));
        assert!(response.body.is_some());
    }

    #[tokio::test]
    async fn test_handle_metadata_request() {
        let handler = MavenFormatHandler::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Maven,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "maven-repo"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/com/example/my-app/maven-metadata.xml".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = handler.handle_request(request).await.unwrap();
        
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, Some("application/xml".to_string()));
        assert!(response.body.is_some());
        
        let xml = String::from_utf8(response.body.unwrap()).unwrap();
        assert!(xml.contains("<groupId>com.example</groupId>"));
        assert!(xml.contains("<artifactId>my-app</artifactId>"));
    }
}