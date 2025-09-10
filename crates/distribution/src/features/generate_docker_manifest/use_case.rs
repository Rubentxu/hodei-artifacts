// crates/distribution/src/features/generate_docker_manifest/use_case.rs

//! Caso de uso para generar manifests Docker

use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn, error, instrument};

use crate::features::generate_docker_manifest::dto::{
    DockerManifestDto, GenerateDockerManifestCommand, GenerateDockerManifestResponse,
};
use crate::features::generate_docker_manifest::ports::{
    DockerManifestGenerator, DockerLayerLister, DockerManifestCache,
    DockerManifestGenerationError, DockerLayerListingError, DockerConfigRetrievalError, DockerCacheError,
};

/// Caso de uso para generar manifests Docker
pub struct GenerateDockerManifestUseCase {
    manifest_generator: Arc<dyn DockerManifestGenerator>,
    layer_lister: Arc<dyn DockerLayerLister>,
    manifest_cache: Arc<dyn DockerManifestCache>,
}

impl GenerateDockerManifestUseCase {
    /// Crear una nueva instancia del caso de uso
    pub fn new(
        manifest_generator: Arc<dyn DockerManifestGenerator>,
        layer_lister: Arc<dyn DockerLayerLister>,
        manifest_cache: Arc<dyn DockerManifestCache>,
    ) -> Self {
        Self {
            manifest_generator,
            layer_lister,
            manifest_cache,
        }
    }

    /// Ejecutar la generación del manifest Docker
    #[instrument(
        skip(self, command),
        fields(
            repository = %command.repository_name,
            tag = %command.tag,
            repository_id = %command.repository_id,
            force_regenerate = command.force_regenerate
        )
    )]
    pub async fn execute(
        &self,
        command: GenerateDockerManifestCommand,
    ) -> Result<GenerateDockerManifestResponse, GenerateDockerManifestError> {
        info!(
            repository = %command.repository_name,
            tag = %command.tag,
            "Starting Docker manifest generation"
        );

        // Validar el comando
        self.validate_command(&command)?;

        // Determinar el tipo de media a usar
        let media_type = self.determine_media_type(&command);

        // Verificar si hay un manifest en caché y no se fuerza la regeneración
        if !command.force_regenerate {
            if let Some(cached_manifest) = self.check_cache(&command, &media_type).await? {
                info!(
                    repository = %command.repository_name,
                    tag = %command.tag,
                    digest = %cached_manifest.digest,
                    "Returning cached Docker manifest"
                );
                
                return Ok(GenerateDockerManifestResponse {
                    manifest: cached_manifest.manifest,
                    digest: cached_manifest.digest,
                    generated_at: cached_manifest.generated_at,
                    from_cache: true,
                    media_type: cached_manifest.media_type,
                });
            }
        }

        // Listar las capas del repositorio
        let layers = self.list_layers(&command).await?;
        info!(
            repository = %command.repository_name,
            tag = %command.tag,
            layer_count = layers.len(),
            "Retrieved Docker layers"
        );

        // Obtener la configuración del contenedor
        let config = self.get_config(&command).await?;
        info!(
            repository = %command.repository_name,
            tag = %command.tag,
            config_digest = %config.digest,
            "Retrieved Docker container config"
        );

        // Generar el manifest
        let manifest = self.generate_manifest(&command, layers, config).await?;
        
        // Calcular el digest del manifest
        let digest = self.calculate_manifest_digest(&manifest)?;

        // Almacenar en caché
        let generated_at = chrono::Utc::now().to_rfc3339();
        self.cache_manifest(&command, &manifest, &digest, &media_type).await?;

        info!(
            repository = %command.repository_name,
            tag = %command.tag,
            digest = %digest,
            media_type = %media_type,
            "Successfully generated Docker manifest"
        );

        Ok(GenerateDockerManifestResponse {
            manifest,
            digest,
            generated_at,
            from_cache: false,
            media_type,
        })
    }

    /// Validar el comando de generación
    fn validate_command(&self, command: &GenerateDockerManifestCommand) -> Result<(), GenerateDockerManifestError> {
        // Validar el nombre del repositorio
        if command.repository_name.is_empty() {
            return Err(GenerateDockerManifestError::ValidationError {
                field: "repository_name".to_string(),
                reason: "Repository name cannot be empty".to_string(),
            });
        }

        // Validar el tag
        if command.tag.is_empty() {
            return Err(GenerateDockerManifestError::ValidationError {
                field: "tag".to_string(),
                reason: "Tag cannot be empty".to_string(),
            });
        }

        // Validar el ID del repositorio
        if command.repository_id.is_empty() {
            return Err(GenerateDockerManifestError::ValidationError {
                field: "repository_id".to_string(),
                reason: "Repository ID cannot be empty".to_string(),
            });
        }

        // Validar el formato del nombre del repositorio Docker
        if !self.is_valid_docker_repository_name(&command.repository_name) {
            return Err(GenerateDockerManifestError::ValidationError {
                field: "repository_name".to_string(),
                reason: "Invalid Docker repository name format".to_string(),
            });
        }

        // Validar el formato del tag
        if !self.is_valid_docker_tag(&command.tag) {
            return Err(GenerateDockerManifestError::ValidationError {
                field: "tag".to_string(),
                reason: "Invalid Docker tag format".to_string(),
            });
        }

        Ok(())
    }

    /// Determinar el tipo de media a usar basado en el comando
    fn determine_media_type(&self, command: &GenerateDockerManifestCommand) -> String {
        command.media_type.clone().unwrap_or_else(|| {
            // Por defecto, usar Docker Manifest V2.2
            "application/vnd.docker.distribution.manifest.v2+json".to_string()
        })
    }

    /// Verificar si hay un manifest en caché
    #[instrument(skip(self, command, media_type))]
    async fn check_cache(
        &self,
        command: &GenerateDockerManifestCommand,
        media_type: &str,
    ) -> Result<Option<crate::features::generate_docker_manifest::ports::CachedDockerManifest>, GenerateDockerManifestError> {
        match self.manifest_cache.get_cached_manifest(&command.repository_name, &command.tag, media_type).await {
            Ok(cached) => Ok(cached),
            Err(e) => {
                warn!(
                    repository = %command.repository_name,
                    tag = %command.tag,
                    error = %e,
                    "Failed to check manifest cache, proceeding with generation"
                );
                Ok(None)
            }
        }
    }

    /// Listar las capas del repositorio
    #[instrument(skip(self, command))]
    async fn list_layers(
        &self,
        command: &GenerateDockerManifestCommand,
    ) -> Result<Vec<crate::features::generate_docker_manifest::ports::DockerLayerInfo>, GenerateDockerManifestError> {
        self.layer_lister
            .list_layers(&command.repository_name, &command.tag)
            .await
            .map_err(|e| match e {
                DockerLayerListingError::RepositoryNotFound { repository } => {
                    GenerateDockerManifestError::RepositoryNotFound { repository }
                }
                DockerLayerListingError::TagNotFound { tag } => {
                    GenerateDockerManifestError::TagNotFound { tag }
                }
                DockerLayerListingError::NoLayersFound { repository, tag } => {
                    GenerateDockerManifestError::NoLayersFound { repository, tag }
                }
                DockerLayerListingError::StorageError(msg) => {
                    GenerateDockerManifestError::StorageError(msg)
                }
                DockerLayerListingError::Internal(msg) => {
                    GenerateDockerManifestError::InternalError(msg)
                }
            })
    }

    /// Obtener la configuración del contenedor
    #[instrument(skip(self, command))]
    async fn get_config(
        &self,
        command: &GenerateDockerManifestCommand,
    ) -> Result<crate::features::generate_docker_manifest::ports::DockerConfigInfo, GenerateDockerManifestError> {
        self.layer_lister
            .get_config(&command.repository_name, &command.tag)
            .await
            .map_err(|e| match e {
                DockerConfigRetrievalError::RepositoryNotFound { repository } => {
                    GenerateDockerManifestError::RepositoryNotFound { repository }
                }
                DockerConfigRetrievalError::TagNotFound { tag } => {
                    GenerateDockerManifestError::TagNotFound { tag }
                }
                DockerConfigRetrievalError::ConfigNotFound { repository, tag } => {
                    GenerateDockerManifestError::ConfigNotFound { repository, tag }
                }
                DockerConfigRetrievalError::StorageError(msg) => {
                    GenerateDockerManifestError::StorageError(msg)
                }
                DockerConfigRetrievalError::Internal(msg) => {
                    GenerateDockerManifestError::InternalError(msg)
                }
            })
    }

    /// Generar el manifest Docker
    #[instrument(skip(self, command, layers, config))]
    async fn generate_manifest(
        &self,
        command: &GenerateDockerManifestCommand,
        layers: Vec<crate::features::generate_docker_manifest::ports::DockerLayerInfo>,
        config: crate::features::generate_docker_manifest::ports::DockerConfigInfo,
    ) -> Result<DockerManifestDto, GenerateDockerManifestError> {
        self.manifest_generator
            .generate_manifest(command, layers, config)
            .await
            .map_err(|e| match e {
                DockerManifestGenerationError::GenerationFailed { reason } => {
                    GenerateDockerManifestError::ManifestGenerationFailed { reason }
                }
                DockerManifestGenerationError::InvalidLayerConfig { reason } => {
                    GenerateDockerManifestError::InvalidLayerConfig { reason }
                }
                DockerManifestGenerationError::InvalidConfig { reason } => {
                    GenerateDockerManifestError::InvalidConfig { reason }
                }
                DockerManifestGenerationError::UnsupportedVersion { version } => {
                    GenerateDockerManifestError::UnsupportedManifestVersion { version }
                }
                DockerManifestGenerationError::Internal(msg) => {
                    GenerateDockerManifestError::InternalError(msg)
                }
            })
    }

    /// Calcular el digest del manifest
    fn calculate_manifest_digest(&self, manifest: &DockerManifestDto) -> Result<String, GenerateDockerManifestError> {
        use sha2::{Sha256, Digest};
        use serde_json;

        let manifest_json = serde_json::to_string(manifest)
            .map_err(|e| GenerateDockerManifestError::SerializationError {
                reason: format!("Failed to serialize manifest: {}", e),
            })?;

        let mut hasher = Sha256::new();
        hasher.update(manifest_json.as_bytes());
        let hash = hasher.finalize();
        
        Ok(format!("sha256:{:x}", hash))
    }

    /// Almacenar el manifest en caché
    #[instrument(skip(self, command, manifest, digest, media_type))]
    async fn cache_manifest(
        &self,
        command: &GenerateDockerManifestCommand,
        manifest: &DockerManifestDto,
        digest: &str,
        media_type: &str,
    ) -> Result<(), GenerateDockerManifestError> {
        if let Err(e) = self.manifest_cache.cache_manifest(
            &command.repository_name,
            &command.tag,
            media_type,
            manifest,
            digest,
        ).await {
            warn!(
                repository = %command.repository_name,
                tag = %command.tag,
                error = %e,
                "Failed to cache manifest, continuing"
            );
        }
        Ok(())
    }

    /// Validar el formato del nombre del repositorio Docker
    fn is_valid_docker_repository_name(&self, name: &str) -> bool {
        // Validar formato básico de nombre de repositorio Docker
        // Debe contener solo caracteres alfanuméricos, guiones, guiones bajos y puntos
        // No puede empezar o terminar con guiones o puntos
        // Debe tener al menos 2 caracteres
        if name.len() < 2 {
            return false;
        }

        // No puede empezar o terminar con guiones o puntos
        if name.starts_with('-') || name.starts_with('.') || 
           name.ends_with('-') || name.ends_with('.') {
            return false;
        }

        // Solo caracteres válidos
        name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    }

    /// Validar el formato del tag Docker
    fn is_valid_docker_tag(&self, tag: &str) -> bool {
        // Los tags Docker no pueden estar vacíos y tienen restricciones
        if tag.is_empty() || tag.len() > 128 {
            return false;
        }

        // No puede empezar con punto o guión
        if tag.starts_with('.') || tag.starts_with('-') {
            return false;
        }

        // Solo caracteres válidos: alfanuméricos, guiones, guiones bajos y puntos
        tag.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    }
}

/// Errores del caso de uso de generación de manifests Docker
#[derive(Debug, Error)]
pub enum GenerateDockerManifestError {
    #[error("Validation error in field '{field}': {reason}")]
    ValidationError { field: String, reason: String },

    #[error("Repository not found: {repository}")]
    RepositoryNotFound { repository: String },

    #[error("Tag not found: {tag}")]
    TagNotFound { tag: String },

    #[error("No layers found for {repository}:{tag}")]
    NoLayersFound { repository: String, tag: String },

    #[error("Config not found for {repository}:{tag}")]
    ConfigNotFound { repository: String, tag: String },

    #[error("Manifest generation failed: {reason}")]
    ManifestGenerationFailed { reason: String },

    #[error("Invalid layer configuration: {reason}")]
    InvalidLayerConfig { reason: String },

    #[error("Invalid config: {reason}")]
    InvalidConfig { reason: String },

    #[error("Unsupported manifest version: {version}")]
    UnsupportedManifestVersion { version: String },

    #[error("Serialization error: {reason}")]
    SerializationError { reason: String },

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Implementaciones mock para testing
#[cfg(test)]
pub mod test {
    use super::*;
    use crate::features::generate_docker_manifest::ports::test::{
        MockDockerManifestGenerator, MockDockerLayerLister, MockDockerManifestCache,
    };

    /// Crear un caso de uso con mocks para testing
    pub fn create_mock_use_case() -> GenerateDockerManifestUseCase {
        let generator = Arc::new(MockDockerManifestGenerator::new());
        let lister = Arc::new(MockDockerLayerLister::new());
        let cache = Arc::new(MockDockerManifestCache::new());

        GenerateDockerManifestUseCase::new(generator, lister, cache)
    }

    /// Crear un caso de uso con mocks configurados para éxito
    pub fn create_mock_use_case_with_success() -> GenerateDockerManifestUseCase {
        let layers = vec![
            crate::features::generate_docker_manifest::ports::DockerLayerInfo {
                digest: "sha256:layer1".to_string(),
                size: 1000,
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                urls: None,
            },
        ];

        let config = crate::features::generate_docker_manifest::ports::DockerConfigInfo {
            digest: "sha256:config1".to_string(),
            size: 500,
            media_type: "application/vnd.docker.container.image.v1+json".to_string(),
            content: b"{}".to_vec(),
        };

        let generator = Arc::new(MockDockerManifestGenerator::new());
        let lister = Arc::new(MockDockerLayerLister::with_layers_and_config(
            "test/repo", "latest", layers, config,
        ));
        let cache = Arc::new(MockDockerManifestCache::new());

        GenerateDockerManifestUseCase::new(generator, lister, cache)
    }

    /// Crear un caso de uso con mocks configurados para fallo
    pub fn create_mock_use_case_with_failure() -> GenerateDockerManifestUseCase {
        let generator = Arc::new(MockDockerManifestGenerator::with_failure());
        let lister = Arc::new(MockDockerLayerLister::with_failure());
        let cache = Arc::new(MockDockerManifestCache::new());

        GenerateDockerManifestUseCase::new(generator, lister, cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generate_docker_manifest::dto::DockerManifestDto;

    #[tokio::test]
    async fn test_validate_command_success() {
        let use_case = create_mock_use_case();
        let command = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let result = use_case.validate_command(&command);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_command_empty_repository() {
        let use_case = create_mock_use_case();
        let command = GenerateDockerManifestCommand {
            repository_name: "".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let result = use_case.validate_command(&command);
        assert!(matches!(result, Err(GenerateDockerManifestError::ValidationError { field, .. }) if field == "repository_name"));
    }

    #[tokio::test]
    async fn test_validate_command_invalid_repository_name() {
        let use_case = create_mock_use_case();
        let command = GenerateDockerManifestCommand {
            repository_name: "-invalid".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let result = use_case.validate_command(&command);
        assert!(matches!(result, Err(GenerateDockerManifestError::ValidationError { field, .. }) if field == "repository_name"));
    }

    #[tokio::test]
    async fn test_validate_command_invalid_tag() {
        let use_case = create_mock_use_case();
        let command = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "-invalid".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let result = use_case.validate_command(&command);
        assert!(matches!(result, Err(GenerateDockerManifestError::ValidationError { field, .. }) if field == "tag"));
    }

    #[tokio::test]
    async fn test_determine_media_type() {
        let use_case = create_mock_use_case();
        
        let command_with_media_type = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: Some("application/vnd.docker.distribution.manifest.list.v2+json".to_string()),
            force_regenerate: false,
        };

        let media_type = use_case.determine_media_type(&command_with_media_type);
        assert_eq!(media_type, "application/vnd.docker.distribution.manifest.list.v2+json");

        let command_without_media_type = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let default_media_type = use_case.determine_media_type(&command_without_media_type);
        assert_eq!(default_media_type, "application/vnd.docker.distribution.manifest.v2+json");
    }

    #[tokio::test]
    async fn test_is_valid_docker_repository_name() {
        let use_case = create_mock_use_case();

        // Nombres válidos
        assert!(use_case.is_valid_docker_repository_name("test"));
        assert!(use_case.is_valid_docker_repository_name("test/repo"));
        assert!(use_case.is_valid_docker_repository_name("test-repo"));
        assert!(use_case.is_valid_docker_repository_name("test_repo"));
        assert!(use_case.is_valid_docker_repository_name("test.repo"));

        // Nombres inválidos
        assert!(!use_case.is_valid_docker_repository_name(""));
        assert!(!use_case.is_valid_docker_repository_name("t")); // muy corto
        assert!(!use_case.is_valid_docker_repository_name("-test"));
        assert!(!use_case.is_valid_docker_repository_name("test-"));
        assert!(!use_case.is_valid_docker_repository_name(".test"));
        assert!(!use_case.is_valid_docker_repository_name("test."));
        assert!(!use_case.is_valid_docker_repository_name("test/repo/invalid"));
    }

    #[tokio::test]
    async fn test_is_valid_docker_tag() {
        let use_case = create_mock_use_case();

        // Tags válidos
        assert!(use_case.is_valid_docker_tag("latest"));
        assert!(use_case.is_valid_docker_tag("v1.0.0"));
        assert!(use_case.is_valid_docker_tag("test-tag"));
        assert!(use_case.is_valid_docker_tag("test_tag"));
        assert!(use_case.is_valid_docker_tag("test.tag"));

        // Tags inválidos
        assert!(!use_case.is_valid_docker_tag(""));
        assert!(!use_case.is_valid_docker_tag(&"a".repeat(129))); // muy largo
        assert!(!use_case.is_valid_docker_tag(".test"));
        assert!(!use_case.is_valid_docker_tag("-test"));
        assert!(!use_case.is_valid_docker_tag("test/tag")); // caracter inválido
    }

    #[tokio::test]
    async fn test_calculate_manifest_digest() {
        let use_case = create_mock_use_case();
        
        let manifest = DockerManifestDto::V2_2(crate::features::generate_docker_manifest::dto::DockerManifestV2_2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            layers: vec![],
            config: crate::features::generate_docker_manifest::dto::Config {
                media_type: "application/json".to_string(),
                size: 100,
                digest: "sha256:config".to_string(),
            },
        });

        let digest = use_case.calculate_manifest_digest(&manifest).unwrap();
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71); // "sha256:" + 64 caracteres hex
    }
}