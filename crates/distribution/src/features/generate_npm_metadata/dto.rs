// crates/distribution/src/features/generate_npm_metadata/dto.rs

//! DTOs para la generación de metadatos npm del repositorio

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Petición para generar metadatos npm del repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateNpmMetadataRequest {
    /// Alcance del paquete npm (opcional)
    pub scope: Option<String>,
    /// Nombre del paquete
    pub package_name: String,
    /// ID del repositorio
    pub repository_id: String,
    /// Forzar regeneración ignorando caché
    pub force_regenerate: bool,
}

/// Respuesta con los metadatos npm generados
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateNpmMetadataResponse {
    /// Metadatos del paquete npm
    pub metadata: NpmPackageMetadataDto,
    /// Timestamp de generación en formato RFC3339
    pub generated_at: String,
    /// Indica si se usó caché
    pub cache_hit: bool,
}

/// Metadatos del paquete npm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPackageMetadataDto {
    /// Nombre del paquete
    pub name: String,
    /// Descripción del paquete
    pub description: Option<String>,
    /// Versión del paquete
    pub version: String,
    /// Palabras clave
    pub keywords: Option<Vec<String>>,
    /// Página principal
    pub homepage: Option<String>,
    /// URL del repositorio
    pub repository: Option<RepositoryDto>,
    /// Autor
    pub author: Option<AuthorDto>,
    /// Licencia
    pub license: Option<String>,
    /// Dependencias
    pub dependencies: Option<std::collections::HashMap<String, String>>,
    /// Dependencias de desarrollo
    pub dev_dependencies: Option<std::collections::HashMap<String, String>>,
    /// Dependencias opcionales
    pub optional_dependencies: Option<std::collections::HashMap<String, String>>,
    /// Dependencias de peer
    pub peer_dependencies: Option<std::collections::HashMap<String, String>>,
    /// Scripts
    pub scripts: Option<std::collections::HashMap<String, String>>,
    /// Archivos principales
    pub main: Option<String>,
    /// Punto de entrada binario
    pub bin: Option<BinDto>,
    /// Archivos incluidos
    pub files: Option<Vec<String>>,
    /// Motores soportados
    pub engines: Option<std::collections::HashMap<String, String>>,
    /// Sistema operativo soportado
    pub os: Option<Vec<String>>,
    /// CPU soportada
    pub cpu: Option<Vec<String>>,
    /// ¿Es privado?
    pub private: Option<bool>,
    /// Configuración de publicación
    publish_config: Option<PublishConfigDto>,
    /// Dist-tags
    pub dist_tags: std::collections::HashMap<String, String>,
    /// Versiones disponibles
    pub versions: Vec<String>,
    /// Tiempo de publicación de cada versión
    pub time: Option<std::collections::HashMap<String, String>>,
    /// Usuarios que han marcado como favorito
    pub users: Option<std::collections::HashMap<String, bool>>,
    /// Enlaces de descarga
    pub dist: Option<DistDto>,
}

/// Información del repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDto {
    /// Tipo de repositorio (git, svn, etc.)
    #[serde(rename = "type")]
    pub repo_type: String,
    /// URL del repositorio
    pub url: String,
    /// Directorio dentro del repositorio
    pub directory: Option<String>,
}

/// Información del autor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuthorDto {
    /// Autor como string
    String(String),
    /// Autor como objeto con nombre y email
    Object {
        name: String,
        email: Option<String>,
        url: Option<String>,
    },
}

/// Configuración binaria
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinDto {
    /// Binario único
    Single(String),
    /// Múltiples binarios
    Multiple(std::collections::HashMap<String, String>),
}

/// Configuración de publicación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishConfigDto {
    /// Registro de publicación
    pub registry: Option<String>,
    /// ¿Ignorar archivos en .npmignore?
    pub ignore: Option<Vec<String>>,
    /// ¿Incluir solo archivos específicos?
    pub include: Option<Vec<String>>,
    /// ¿Es accesible públicamente?
    pub access: Option<String>,
    /// Tag de publicación
    pub tag: Option<String>,
}

/// Información de distribución
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistDto {
    /// Integridad del archivo (sha512, sha1, etc.)
    pub integrity: Option<String>,
    /// URL de descarga del tarball
    pub tarball: String,
    /// Número de archivos en el tarball
    pub file_count: Option<u64>,
    /// Tamaño descomprimido
    pub unpacked_size: Option<u64>,
    /// Suma de verificación
    pub shasum: Option<String>,
    /// Tamaño del tarball
    pub size: Option<u64>,
}

/// Errores específicos de la feature
#[derive(Debug, Error)]
pub enum GenerateNpmMetadataError {
    #[error("Package not found: {package_name} in repository {repository_id}")]
    PackageNotFound {
        package_name: String,
        repository_id: String,
    },
    
    #[error("Invalid package name: {name}")]
    InvalidPackageName { name: String },
    
    #[error("Metadata generation failed: {reason}")]
    MetadataGenerationFailed { reason: String },
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Permission denied for package {package_name} in repository {repository_id}")]
    PermissionDenied {
        package_name: String,
        repository_id: String,
    },
}

/// Validación de nombres de paquetes npm
pub fn validate_npm_package_name(name: &str) -> Result<(), GenerateNpmMetadataError> {
    if name.is_empty() {
        return Err(GenerateNpmMetadataError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    
    // Validar longitud
    if name.len() > 214 {
        return Err(GenerateNpmMetadataError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    
    // Validar caracteres permitidos
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@' || c == '/') {
        return Err(GenerateNpmMetadataError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    
    // Validar que no empiece con punto o guión
    if name.starts_with('.') || name.starts_with('-') {
        return Err(GenerateNpmMetadataError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    
    // Validar que no termine con punto
    if name.ends_with('.') {
        return Err(GenerateNpmMetadataError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    
    // Validar scoped packages
    if name.contains('/') {
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() != 2 {
            return Err(GenerateNpmMetadataError::InvalidPackageName {
                name: name.to_string(),
            });
        }
        
        let scope = parts[0];
        let package = parts[1];
        
        // El scope debe empezar con @
        if !scope.starts_with('@') {
            return Err(GenerateNpmMetadataError::InvalidPackageName {
                name: name.to_string(),
            });
        }
        
        // Validar el nombre del paquete dentro del scope
        if package.is_empty() || package.starts_with('.') || package.starts_with('-') {
            return Err(GenerateNpmMetadataError::InvalidPackageName {
                name: name.to_string(),
            });
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_npm_package_name_valid() {
        assert!(validate_npm_package_name("express").is_ok());
        assert!(validate_npm_package_name("lodash").is_ok());
        assert!(validate_npm_package_name("@angular/core").is_ok());
        assert!(validate_npm_package_name("@types/node").is_ok());
        assert!(validate_npm_package_name("my-package").is_ok());
        assert!(validate_npm_package_name("my_package").is_ok());
        assert!(validate_npm_package_name("my.package").is_ok());
    }

    #[test]
    fn test_validate_npm_package_name_invalid() {
        assert!(validate_npm_package_name("").is_err());
        assert!(validate_npm_package_name(".package").is_err());
        assert!(validate_npm_package_name("-package").is_err());
        assert!(validate_npm_package_name("package.").is_err());
        assert!(validate_npm_package_name("package with spaces").is_err());
        assert!(validate_npm_package_name("package@invalid").is_err());
        assert!(validate_npm_package_name("@scope").is_err());
        assert!(validate_npm_package_name("@scope/").is_err());
        assert!(validate_npm_package_name("@scope/.package").is_err());
        assert!(validate_npm_package_name("@scope/-package").is_err());
    }

    #[test]
    fn test_npm_package_metadata_dto_creation() {
        let metadata = NpmPackageMetadataDto {
            name: "test-package".to_string(),
            description: Some("A test package".to_string()),
            version: "1.0.0".to_string(),
            keywords: Some(vec!["test".to_string(), "package".to_string()]),
            homepage: Some("https://example.com".to_string()),
            repository: Some(RepositoryDto {
                repo_type: "git".to_string(),
                url: "https://github.com/user/repo.git".to_string(),
                directory: None,
            }),
            author: Some(AuthorDto::String("Test Author".to_string())),
            license: Some("MIT".to_string()),
            dependencies: None,
            dev_dependencies: None,
            optional_dependencies: None,
            peer_dependencies: None,
            scripts: None,
            main: Some("index.js".to_string()),
            bin: None,
            files: None,
            engines: None,
            os: None,
            cpu: None,
            private: Some(false),
            publish_config: None,
            dist_tags: std::collections::HashMap::from([
                ("latest".to_string(), "1.0.0".to_string()),
                ("beta".to_string(), "1.1.0-beta.1".to_string()),
            ]),
            versions: vec!["1.0.0".to_string(), "1.1.0-beta.1".to_string()],
            time: None,
            users: None,
            dist: Some(DistDto {
                integrity: Some("sha512-abc123".to_string()),
                tarball: "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
                file_count: Some(10),
                unpacked_size: Some(1024),
                shasum: Some("def456".to_string()),
                size: Some(512),
            }),
        };

        assert_eq!(metadata.name, "test-package");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.dist_tags.len(), 2);
        assert_eq!(metadata.versions.len(), 2);
        assert!(metadata.dist.is_some());
    }

    #[test]
    fn test_generate_npm_metadata_request_creation() {
        let request = GenerateNpmMetadataRequest {
            scope: Some("@myorg".to_string()),
            package_name: "my-package".to_string(),
            repository_id: "npm-registry".to_string(),
            force_regenerate: false,
        };

        assert_eq!(request.scope, Some("@myorg".to_string()));
        assert_eq!(request.package_name, "my-package");
        assert_eq!(request.repository_id, "npm-registry");
        assert!(!request.force_regenerate);
    }

    #[test]
    fn test_generate_npm_metadata_response_creation() {
        let metadata = NpmPackageMetadataDto {
            name: "test-package".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            keywords: None,
            homepage: None,
            repository: None,
            author: None,
            license: None,
            dependencies: None,
            dev_dependencies: None,
            optional_dependencies: None,
            peer_dependencies: None,
            scripts: None,
            main: None,
            bin: None,
            files: None,
            engines: None,
            os: None,
            cpu: None,
            private: None,
            publish_config: None,
            dist_tags: std::collections::HashMap::new(),
            versions: vec!["1.0.0".to_string()],
            time: None,
            users: None,
            dist: None,
        };

        let response = GenerateNpmMetadataResponse {
            metadata,
            generated_at: "2024-01-01T12:00:00Z".to_string(),
            cache_hit: true,
        };

        assert_eq!(response.metadata.name, "test-package");
        assert!(response.cache_hit);
        assert_eq!(response.generated_at, "2024-01-01T12:00:00Z");
    }
}