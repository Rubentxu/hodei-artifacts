// crates/distribution/src/features/generate_docker_manifest/dto.rs

//! DTOs para la generación de manifests Docker

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Petición para generar un manifest Docker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateDockerManifestRequest {
    /// Nombre del repositorio Docker (ej: "library/nginx")
    pub repository_name: String,
    /// Tag del manifest (ej: "latest", "1.21")
    pub tag: String,
    /// ID del repositorio en el sistema
    pub repository_id: String,
    /// Tipo de manifest (opcional, por defecto "application/vnd.docker.distribution.manifest.v2+json")
    pub media_type: Option<String>,
    /// Forzar regeneración ignorando caché
    pub force_regenerate: bool,
}

/// Respuesta con el manifest Docker generado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateDockerManifestResponse {
    /// Manifest Docker generado
    pub manifest: DockerManifestDto,
    /// Digest del manifest (SHA256)
    pub digest: String,
    /// Timestamp de generación en formato RFC3339
    pub generated_at: String,
    /// Indica si se usó caché
    pub cache_hit: bool,
    /// Tipo de media del manifest
    pub media_type: String,
}

/// Manifest Docker V2
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "schemaVersion")]
pub enum DockerManifestDto {
    /// Manifest V2.1 (firmado)
    #[serde(rename = "1")]
    V2_1(DockerManifestV2_1),
    /// Manifest V2.2 (lista de manifests)
    #[serde(rename = "2")]
    V2_2(DockerManifestV2_2),
}

/// Manifest Docker V2.1 (firmado)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestV2_1 {
    /// Versión del esquema (siempre 1)
    pub schema_version: u32,
    /// Nombre del repositorio
    pub name: String,
    /// Tag del manifest
    pub tag: String,
    /// Arquitectura (ej: "amd64", "arm64")
    pub architecture: String,
    /// Hash de la imagen
    pub fs_layers: Vec<FsLayer>,
    /// Historia de la imagen
    pub history: Vec<HistoryEntry>,
    /// Firma del manifest
    pub signatures: Vec<Signature>,
}

/// Manifest Docker V2.2 (lista de manifests)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestV2_2 {
    /// Versión del esquema (siempre 2)
    pub schema_version: u32,
    /// Tipo de media
    pub media_type: String,
    /// Capas del manifest
    pub layers: Vec<Layer>,
    /// Configuración del contenedor
    pub config: Config,
}

/// Capa del sistema de archivos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsLayer {
    /// Blob sum (digest de la capa)
    #[serde(rename = "blobSum")]
    pub blob_sum: String,
}

/// Entrada del historial
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Hash v1 de la imagen
    #[serde(rename = "v1Compatibility")]
    pub v1_compatibility: String,
}

/// Firma del manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// Cabecera de la firma
    pub header: SignatureHeader,
    /// Firma en sí
    pub signature: String,
    /// Certificado de protección
    #[serde(rename = "protected")]
    pub protected: String,
}

/// Cabecera de la firma
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureHeader {
    /// Algoritmo de firma
    pub alg: String,
    /// JSON Web Key
    pub jwk: Jwk,
}

/// JSON Web Key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    /// Tipo de clave
    #[serde(rename = "kty")]
    pub kty: String,
    /// Curva elíptica
    #[serde(rename = "crv")]
    pub crv: String,
    /// Coordenada x
    pub x: String,
    /// Coordenada y
    pub y: String,
}

/// Capa de un manifest V2.2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Tipo de media
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// Tamaño en bytes
    pub size: u64,
    /// Digest de la capa
    pub digest: String,
    /// URLs de descarga (opcional)
    pub urls: Option<Vec<String>>,
}

/// Configuración del contenedor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Tipo de media
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// Tamaño en bytes
    pub size: u64,
    /// Digest de la configuración
    pub digest: String,
}

/// Lista de manifests (para manifests de lista)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestList {
    /// Versión del esquema
    pub schema_version: u32,
    /// Tipo de media
    pub media_type: String,
    /// Lista de manifests
    pub manifests: Vec<ManifestDescriptor>,
}

/// Descriptor de manifest en una lista
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDescriptor {
    /// Tipo de media
    #[serde(rename = "mediaType")]
    pub media_type: String,
    /// Tamaño en bytes
    pub size: u64,
    /// Digest del manifest
    pub digest: String,
    /// Plataforma específica
    pub platform: Option<Platform>,
}

/// Plataforma específica para un manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    /// Arquitectura (ej: "amd64", "arm64", "386")
    pub architecture: String,
    /// Sistema operativo (ej: "linux", "windows", "darwin")
    pub os: String,
    /// Versión del OS (opcional)
    pub os_version: Option<String>,
    /// Variantes de la arquitectura (opcional)
    pub variant: Option<String>,
    /// Características del CPU (opcional)
    pub features: Option<Vec<String>>,
}

/// Comando para generar un manifest Docker
#[derive(Debug, Clone)]
pub struct GenerateDockerManifestCommand {
    /// Nombre del repositorio
    pub repository_name: String,
    /// Tag del manifest
    pub tag: String,
    /// ID del repositorio
    pub repository_id: String,
    /// Tipo de media (opcional)
    pub media_type: Option<String>,
    /// Forzar regeneración
    pub force_regenerate: bool,
}

/// Errores específicos de la feature
#[derive(Debug, Error)]
pub enum GenerateDockerManifestError {
    #[error("Invalid repository name: {name}")]
    InvalidRepositoryName { name: String },
    
    #[error("Invalid tag: {tag}")]
    InvalidTag { tag: String },
    
    #[error("Manifest not found: {repository}:{tag}")]
    ManifestNotFound { repository: String, tag: String },
    
    #[error("Manifest generation failed: {reason}")]
    ManifestGenerationFailed { reason: String },
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Unsupported media type: {media_type}")]
    UnsupportedMediaType { media_type: String },
    
    #[error("Permission denied for repository {repository}")]
    PermissionDenied { repository: String },
}

/// Validación de nombres de repositorios Docker
pub fn validate_docker_repository_name(name: &str) -> Result<(), GenerateDockerManifestError> {
    if name.is_empty() {
        return Err(GenerateDockerManifestError::InvalidRepositoryName {
            name: name.to_string(),
        });
    }
    
    // Longitud máxima: 255 caracteres
    if name.len() > 255 {
        return Err(GenerateDockerManifestError::InvalidRepositoryName {
            name: name.to_string(),
        });
    }
    
    // Validar caracteres permitidos: alfanuméricos, guiones, guiones bajos, puntos, barras
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/') {
        return Err(GenerateDockerManifestError::InvalidRepositoryName {
            name: name.to_string(),
        });
    }
    
    // No puede empezar o terminar con guiones o puntos
    if name.starts_with('-') || name.starts_with('.') || name.ends_with('-') || name.ends_with('.') {
        return Err(GenerateDockerManifestError::InvalidRepositoryName {
            name: name.to_string(),
        });
    }
    
    // Validar componentes separados por barras
    for component in name.split('/') {
        if component.is_empty() {
            return Err(GenerateDockerManifestError::InvalidRepositoryName {
                name: name.to_string(),
            });
        }
        
        // Cada componente debe tener al menos 2 caracteres
        if component.len() < 2 {
            return Err(GenerateDockerManifestError::InvalidRepositoryName {
                name: name.to_string(),
            });
        }
        
        // No puede tener dobles barras
        if component.contains("//") {
            return Err(GenerateDockerManifestError::InvalidRepositoryName {
                name: name.to_string(),
            });
        }
    }
    
    Ok(())
}

/// Validación de tags Docker
pub fn validate_docker_tag(tag: &str) -> Result<(), GenerateDockerManifestError> {
    if tag.is_empty() {
        return Err(GenerateDockerManifestError::InvalidTag {
            tag: tag.to_string(),
        });
    }
    
    // Longitud máxima: 128 caracteres
    if tag.len() > 128 {
        return Err(GenerateDockerManifestError::InvalidTag {
            tag: tag.to_string(),
        });
    }
    
    // Validar caracteres permitidos: alfanuméricos, guiones, guiones bajos, puntos
    if !tag.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(GenerateDockerManifestError::InvalidTag {
            tag: tag.to_string(),
        });
    }
    
    // Tags especiales permitidas
    let special_tags = ["latest", "stable", "edge", "nightly", "dev"];
    if special_tags.contains(&tag) {
        return Ok(());
    }
    
    // Validar formato semver si parece una versión
    if tag.contains('.') {
        let parts: Vec<&str> = tag.split('.').collect();
        if parts.len() >= 2 && parts.iter().all(|part| {
            part.chars().all(|c| c.is_numeric() || c == '-' || c == '+' || c.is_alphabetic())
        }) {
            return Ok(());
        }
    }
    
    Ok(())
}

/// Tipos de media soportados para manifests Docker
pub const SUPPORTED_DOCKER_MEDIA_TYPES: &[&str] = &[
    "application/vnd.docker.distribution.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v1+prettyjws",
    "application/vnd.docker.distribution.manifest.v2+json",
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.oci.image.index.v1+json",
];

/// Validar tipo de media
pub fn validate_docker_media_type(media_type: &str) -> Result<(), GenerateDockerManifestError> {
    if SUPPORTED_DOCKER_MEDIA_TYPES.contains(&media_type) {
        Ok(())
    } else {
        Err(GenerateDockerManifestError::UnsupportedMediaType {
            media_type: media_type.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_docker_repository_name_valid() {
        assert!(validate_docker_repository_name("nginx").is_ok());
        assert!(validate_docker_repository_name("library/nginx").is_ok());
        assert!(validate_docker_repository_name("myorg/myapp").is_ok());
        assert!(validate_docker_repository_name("registry.example.com/myorg/myapp").is_ok());
        assert!(validate_docker_repository_name("my-app").is_ok());
        assert!(validate_docker_repository_name("my_app").is_ok());
        assert!(validate_docker_repository_name("my.app").is_ok());
    }

    #[test]
    fn test_validate_docker_repository_name_invalid() {
        assert!(validate_docker_repository_name("").is_err());
        assert!(validate_docker_repository_name("-invalid").is_err());
        assert!(validate_docker_repository_name("invalid-").is_err());
        assert!(validate_docker_repository_name(".invalid").is_err());
        assert!(validate_docker_repository_name("invalid.").is_err());
        assert!(validate_docker_repository_name("a").is_err()); // Too short
        assert!(validate_docker_repository_name("a/b").is_err()); // Component too short
        assert!(validate_docker_repository_name("my//repo").is_err()); // Double slash
        assert!(validate_docker_repository_name("my/repo/").is_err()); // Trailing slash
        assert!(validate_docker_repository_name(&"a".repeat(256)).is_err()); // Too long
    }

    #[test]
    fn test_validate_docker_tag_valid() {
        assert!(validate_docker_tag("latest").is_ok());
        assert!(validate_docker_tag("stable").is_ok());
        assert!(validate_docker_tag("1.0.0").is_ok());
        assert!(validate_docker_tag("v1.2.3").is_ok());
        assert!(validate_docker_tag("1.0.0-beta.1").is_ok());
        assert!(validate_docker_tag("my-tag").is_ok());
        assert!(validate_docker_tag("my_tag").is_ok());
        assert!(validate_docker_tag("my.tag").is_ok());
    }

    #[test]
    fn test_validate_docker_tag_invalid() {
        assert!(validate_docker_tag("").is_err());
        assert!(validate_docker_tag("invalid tag").is_err()); // Space
        assert!(validate_docker_tag("invalid@tag").is_err()); // @
        assert!(validate_docker_tag("invalid/tag").is_err()); // Slash
        assert!(validate_docker_tag(&"a".repeat(129)).is_err()); // Too long
    }

    #[test]
    fn test_docker_manifest_dto_creation() {
        let manifest = DockerManifestDto::V2_2(DockerManifestV2_2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            layers: vec![
                Layer {
                    media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                    size: 1234567,
                    digest: "sha256:abc123def456".to_string(),
                    urls: None,
                },
            ],
            config: Config {
                media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                size: 2345678,
                digest: "sha256:def456ghi789".to_string(),
            },
        });

        match manifest {
            DockerManifestDto::V2_2(ref v2_2) => {
                assert_eq!(v2_2.schema_version, 2);
                assert_eq!(v2_2.layers.len(), 1);
                assert_eq!(v2_2.layers[0].size, 1234567);
            }
            _ => panic!("Expected V2_2 manifest"),
        }
    }

    #[test]
    fn test_generate_docker_manifest_request_creation() {
        let request = GenerateDockerManifestRequest {
            repository_name: "library/nginx".to_string(),
            tag: "latest".to_string(),
            repository_id: "docker-hub".to_string(),
            media_type: Some("application/vnd.docker.distribution.manifest.v2+json".to_string()),
            force_regenerate: false,
        };

        assert_eq!(request.repository_name, "library/nginx");
        assert_eq!(request.tag, "latest");
        assert_eq!(request.repository_id, "docker-hub");
        assert!(request.media_type.is_some());
        assert!(!request.force_regenerate);
    }

    #[test]
    fn test_generate_docker_manifest_response_creation() {
        let manifest = DockerManifestDto::V2_2(DockerManifestV2_2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            layers: vec![],
            config: Config {
                media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                size: 1000,
                digest: "sha256:config123".to_string(),
            },
        });

        let response = GenerateDockerManifestResponse {
            manifest: manifest.clone(),
            digest: "sha256:manifest123".to_string(),
            generated_at: "2024-01-01T12:00:00Z".to_string(),
            cache_hit: false,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
        };

        assert_eq!(response.digest, "sha256:manifest123");
        assert!(!response.cache_hit);
        assert_eq!(response.media_type, "application/vnd.docker.distribution.manifest.v2+json");
    }
}