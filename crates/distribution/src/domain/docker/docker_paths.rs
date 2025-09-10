
// crates/distribution/src/domain/docker/docker_paths.rs

use crate::domain::error::{FormatError, DistributionResult};
use shared::models::PackageCoordinates;
use std::collections::HashMap;

/// Información extraída de un path Docker Registry V2
#[derive(Debug, Clone)]
pub struct DockerPathInfo {
    pub namespace: String,
    pub repository: String,
    pub reference: Option<String>, // tag o digest
    pub digest: Option<String>,
    pub media_type: Option<String>,
    pub is_manifest: bool,
    pub is_blob: bool,
    pub is_catalog: bool,
    pub is_tags: bool,
    pub coordinates: Option<PackageCoordinates>,
}

/// Parser de paths Docker Registry V2 API
pub struct DockerPathParser;

impl DockerPathParser {
    pub fn new() -> Self {
        Self
    }

    /// Parsea un path Docker Registry V2 y extrae información relevante
    /// 
    /// Formatos soportados (Docker Registry V2 API):
    /// - /v2/                                    # API base
    /// - /v2/_catalog                           # Listado de repositorios
    /// - /v2/{name}/manifests/{reference}       # Manifest (tag o digest)
    /// - /v2/{name}/blobs/{digest}              # Blob por digest
    /// - /v2/{name}/blobs/uploads/              # Upload de blob
    /// - /v2/{name}/tags/list                   # Listado de tags
    pub fn parse_path(&self, path: &str) -> DistributionResult<DockerPathInfo> {
        let clean_path = path.trim_start_matches('/');
        let components: Vec<&str> = clean_path.split('/').collect();
        
        if components.is_empty() {
            return Err(FormatError::DockerError("Empty path".to_string()).into());
        }

        // Verificar que es API v2
        if components[0] != "v2" {
            return Err(FormatError::DockerError("Docker Registry V2 API required".to_string()).into());
        }

        match components.as_slice() {
            // API base: /v2/
            ["v2"] => {
                Ok(DockerPathInfo {
                    namespace: "docker".to_string(),
                    repository: "registry".to_string(),
                    reference: None,
                    digest: None,
                    media_type: None,
                    is_manifest: false,
                    is_blob: false,
                    is_catalog: false,
                    is_tags: false,
                    coordinates: None,
                })
            }
            // Catalog: /v2/_catalog
            ["v2", "_catalog"] => {
                Ok(DockerPathInfo {
                    namespace: "docker".to_string(),
                    repository: "catalog".to_string(),
                    reference: None,
                    digest: None,
                    media_type: None,
                    is_manifest: false,
                    is_blob: false,
                    is_catalog: true,
                    is_tags: false,
                    coordinates: None,
                })
            }
            // Manifest: /v2/{name}/manifests/{reference}
            ["v2", name, "manifests", reference] => {
                self.parse_manifest_path(name, reference)
            }
            // Blob: /v2/{name}/blobs/{digest}
            ["v2", name, "blobs", digest] => {
                self.parse_blob_path(name, digest)
            }
            // Blob upload: /v2/{name}/blobs/uploads/
            ["v2", name, "blobs", "uploads"] => {
                self.parse_blob_upload_path(name)
            }
            // Tags list: /v2/{name}/tags/list
            ["v2", name, "tags", "list"] => {
                self.parse_tags_list_path(name)
            }
            _ => {
                Err(FormatError::DockerError(format!("Unsupported Docker Registry V2 path: {}", path)).into())
            }
        }
    }

    /// Parsea un path de manifest
    fn parse_manifest_path(&self, name: &str, reference: &str) -> DistributionResult<DockerPathInfo> {
        // Validar formato del nombre (namespace/repository)
        let (namespace, repository) = self.parse_docker_name(name)?;
        
        // Determinar si es tag o digest
        let (reference_type, digest) = if self.is_digest(reference) {
            ("digest", Some(reference.to_string()))
        } else {
            ("tag", None)
        };

        let coordinates = PackageCoordinates {
            group: namespace.clone(),
            name: repository.clone(),
            version: reference.to_string(),
            classifier: Some(reference_type.to_string()),
            extension: None,
        };

        Ok(DockerPathInfo {
            namespace,
            repository,
            reference: Some(reference.to_string()),
            digest,
            media_type: None, // Se determinará según el request
            is_manifest: true,
            is_blob: false,
            is_catalog: false,
            is_tags: false,
            coordinates: Some(coordinates),
        })
    }

    /// Parsea un path de blob
    fn parse_blob_path(&self, name: &str, digest: &str) -> DistributionResult<DockerPathInfo> {
        // Validar formato del nombre
        let (namespace, repository) = self.parse_docker_name(name)?;
        
        // Validar formato del digest
        if !self.is_digest(digest) {
            return Err(FormatError::DockerError(format!("Invalid digest format: {}", digest)).into());
        }

        let coordinates = PackageCoordinates {
            group: namespace.clone(),
            name: repository.clone(),
            version: digest.to_string(),
            classifier: Some("blob".to_string()),
            extension: None,
        };

        Ok(DockerPathInfo {
            namespace,
            repository,
            reference: None,
            digest: Some(digest.to_string()),
            media_type: None,
            is_manifest: false,
            is_blob: true,
            is_catalog: false,
            is_tags: false,
            coordinates: Some(coordinates),
        })
    }

    /// Parsea un path de upload de blob
    fn parse_blob_upload_path(&self, name: &str) -> DistributionResult<DockerPathInfo> {
        let (namespace, repository) = self.parse_docker_name(name)?;

        Ok(DockerPathInfo {
            namespace,
            repository,
            reference: None,
            digest: None,
            media_type: None,
            is_manifest: false,
            is_blob: true, // Es un upload de blob
            is_catalog: false,
            is_tags: false,
            coordinates: None,
        })
    }

    /// Parsea un path de listado de tags
    fn parse_tags_list_path(&self, name: &str) -> DistributionResult<DockerPathInfo> {
        let (namespace, repository) = self.parse_docker_name(name)?;

        Ok(DockerPathInfo {
            namespace,
            repository,
            reference: None,
            digest: None,
            media_type: None,
            is_manifest: false,
            is_blob: false,
            is_catalog: false,
            is_tags: true,
            coordinates: None,
        })
    }

    /// Parsea y valida un nombre Docker (namespace/repository)
    fn parse_docker_name(&self, name: &str) -> DistributionResult<(String, String)> {
        if name.is_empty() {
            return Err(FormatError::DockerError("Docker name cannot be empty".to_string()).into());
        }

        // Docker names pueden tener formato: namespace/repository o solo repository
        let parts: Vec<&str> = name.split('/').collect();
        
        match parts.len() {
            1 => {
                // Solo repository (namespace implícito es "library")
                Ok(("library".to_string(), name.to_string()))
            }
            2 => {
                // namespace/repository
                let namespace = parts[0];
                let repository = parts[1];
                
                self.validate_docker_name_component(namespace, "namespace")?;
                self.validate_docker_name_component(repository, "repository")?;
                
                Ok((namespace.to_string(), repository.to_string()))
            }
            _ => {
                Err(FormatError::DockerError(format!("Invalid Docker name format: {}. Expected namespace/repository or repository", name)).into())
            }
        }
    }

    /// Valida un componente del nombre Docker
    fn validate_docker_name_component(&self, component: &str, component_type: &str) -> DistributionResult<()> {
        if component.is_empty() {
            return Err(FormatError::DockerError(format!("Docker {} cannot be empty", component_type)).into());
        }

        if component.len() > 255 {
            return Err(FormatError::DockerError(format!("Docker {} too long (max 255 characters)", component_type)).into());
        }

        // Reglas de validación Docker:
        // - Solo caracteres alfanuméricos, guiones, guiones bajos y puntos
        // - No puede empezar ni terminar con guión o punto
        // - No puede tener guiones o puntos consecutivos
        if !component.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(FormatError::DockerError(format!("Docker {} can only contain letters, numbers, '-', '_', and '.'", component_type)).into());
        }

        if component.starts_with('-') || component.starts_with('.') || 
           component.ends_with('-') || component.ends_with('.') {
            return Err(FormatError::DockerError(format!("Docker {} cannot start or end with '-' or '.'", component_type)).into());
        }

        // Verificar guiones/puntos consecutivos
        if component.contains("--") || component.contains("..") || 
           component.contains("-.") || component.contains(".-") {
            return Err(FormatError::DockerError(format!("Docker {} cannot have consecutive '-', '.', '-.', or '.-'", component_type)).into());
        }

        Ok(())
    }

    /// Determina si una referencia es un digest (sha256:...)
    fn is_digest(&self, reference: &str) -> bool {
        reference.starts_with("sha256:") || 
        reference.starts_with("sha512:") ||
        reference.starts_with("sha1:") ||
        reference.contains(':') // Generalmente los digests tienen formato algorithm:hash
    }

    /// Construye un path para manifest
    pub fn build_manifest_path(&self, name: &str, reference: &str) -> String {
        format!("/v2/{}/manifests/{}", name, reference)
    }

    /// Construye un path para blob
    pub fn build_blob_path(&self, name: &str, digest: &str) -> String {
        format!("/v2/{}/blobs/{}", name, digest)
    }

    /// Construye un path para upload de blob
    pub fn build_blob_upload_path(&self, name: &str) -> String {
        format!("/v2/{}/blobs/uploads/", name)
    }

    /// Construye un path para listado de tags
    pub fn build_tags_list_path(&self, name: &str) -> String {
        format!("/v2/{}/tags/list", name)
    }

    /// Construye un path para catalog
    pub fn build_catalog_path(&self) -> String {
        "/v2/_catalog".to_string()
    }

    /// Construye un path base
    pub fn build_base_path(&self) -> String {
        "/v2/".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest_path_with_tag() {
        let parser = DockerPathParser::new();
        
        let path = "/v2/library/nginx/manifests/latest";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.namespace, "library");
        assert_eq!(info.repository, "nginx");
        assert_eq!(info.reference, Some("latest".to_string()));
        assert!(info.is_manifest);
        assert!(!info.is_blob);
        assert!(info.coordinates.is_some());
    }

    #[test]
    fn test_parse_manifest_path_with_digest() {
        let parser = DockerPathParser::new();
        
        let path = "/v2/library/nginx/manifests/sha256:1234567890abcdef";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.namespace, "library");
        assert_eq!(info.repository, "nginx");
        assert_eq!(info.reference, Some("sha256:1234567890abcdef".to_string()));
        assert_eq!(info.digest, Some("sha256:1234567890abcdef".to_string()));
        assert!(info.is_manifest);
        assert!(!info.is_blob);
    }

    #[test]
    fn test_parse_blob_path() {
        let parser = DockerPathParser::new();
        
        let path = "/v2/library/nginx/blobs/sha256:1234567890abcdef";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.namespace, "library");
        assert_eq!(info.repository, "nginx");
        assert_eq!(info.digest, Some("sha256:1234567890abcdef".to_string()));
        assert!(!info.is_manifest);
        assert!(info.is_blob);
        assert!(info.coordinates.is_some());
    }
}