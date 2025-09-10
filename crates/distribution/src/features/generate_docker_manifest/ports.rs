// crates/distribution/src/features/generate_docker_manifest/ports.rs

//! Puertos segregados para la generación de manifests Docker

use async_trait::async_trait;
use thiserror::Error;

use crate::features::generate_docker_manifest::dto::{
    DockerManifestDto, GenerateDockerManifestCommand, GenerateDockerManifestResponse,
};

/// Generador de manifests Docker
#[async_trait]
pub trait DockerManifestGenerator: Send + Sync {
    /// Generar un manifest Docker a partir de las capas y configuración
    async fn generate_manifest(
        &self,
        command: &GenerateDockerManifestCommand,
        layers: Vec<DockerLayerInfo>,
        config: DockerConfigInfo,
    ) -> Result<DockerManifestDto, DockerManifestGenerationError>;
}

/// Listador de capas y configuraciones Docker
#[async_trait]
pub trait DockerLayerLister: Send + Sync {
    /// Listar todas las capas disponibles para un repositorio y tag
    async fn list_layers(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<Vec<DockerLayerInfo>, DockerLayerListingError>;
    
    /// Obtener la configuración del contenedor para un repositorio y tag
    async fn get_config(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<DockerConfigInfo, DockerConfigRetrievalError>;
}

/// Caché de manifests Docker
#[async_trait]
pub trait DockerManifestCache: Send + Sync {
    /// Obtener un manifest del caché
    async fn get_cached_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        media_type: &str,
    ) -> Result<Option<CachedDockerManifest>, DockerCacheError>;
    
    /// Almacenar un manifest en caché
    async fn cache_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        media_type: &str,
        manifest: &DockerManifestDto,
        digest: &str,
    ) -> Result<(), DockerCacheError>;
    
    /// Invalidar el caché para un repositorio y tag específicos
    async fn invalidate_cache(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<(), DockerCacheError>;
}

/// Información de una capa Docker
#[derive(Debug, Clone)]
pub struct DockerLayerInfo {
    /// Digest de la capa (ej: "sha256:abc123...")
    pub digest: String,
    /// Tamaño en bytes
    pub size: u64,
    /// Tipo de media
    pub media_type: String,
    /// URLs de descarga (opcional)
    pub urls: Option<Vec<String>>,
}

/// Información de configuración del contenedor Docker
#[derive(Debug, Clone)]
pub struct DockerConfigInfo {
    /// Digest de la configuración
    pub digest: String,
    /// Tamaño en bytes
    pub size: u64,
    /// Tipo de media
    pub media_type: String,
    /// Contenido de la configuración (JSON)
    pub content: Vec<u8>,
}

/// Manifest Docker en caché
#[derive(Debug, Clone)]
pub struct CachedDockerManifest {
    /// Manifest Docker
    pub manifest: DockerManifestDto,
    /// Digest del manifest
    pub digest: String,
    /// Timestamp de generación
    pub generated_at: String,
    /// Tipo de media
    pub media_type: String,
}

/// Errores de generación de manifests Docker
#[derive(Debug, Error)]
pub enum DockerManifestGenerationError {
    #[error("Failed to generate manifest: {reason}")]
    GenerationFailed { reason: String },
    
    #[error("Invalid layer configuration: {reason}")]
    InvalidLayerConfig { reason: String },
    
    #[error("Invalid config: {reason}")]
    InvalidConfig { reason: String },
    
    #[error("Unsupported manifest version: {version}")]
    UnsupportedVersion { version: String },
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errores de listado de capas Docker
#[derive(Debug, Error)]
pub enum DockerLayerListingError {
    #[error("Repository not found: {repository}")]
    RepositoryNotFound { repository: String },
    
    #[error("Tag not found: {tag}")]
    TagNotFound { tag: String },
    
    #[error("No layers found for {repository}:{tag}")]
    NoLayersFound { repository: String, tag: String },
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errores de recuperación de configuración Docker
#[derive(Debug, Error)]
pub enum DockerConfigRetrievalError {
    #[error("Repository not found: {repository}")]
    RepositoryNotFound { repository: String },
    
    #[error("Tag not found: {tag}")]
    TagNotFound { tag: String },
    
    #[error("Config not found for {repository}:{tag}")]
    ConfigNotFound { repository: String, tag: String },
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errores de caché Docker
#[derive(Debug, Error)]
pub enum DockerCacheError {
    #[error("Cache storage error: {0}")]
    StorageError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Cache unavailable")]
    CacheUnavailable,
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Implementaciones mock para testing
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;

    /// Mock para DockerManifestGenerator
    pub struct MockDockerManifestGenerator {
        pub should_fail: bool,
        pub generated_manifests: Mutex<Vec<(GenerateDockerManifestCommand, Vec<DockerLayerInfo>, DockerConfigInfo)>>,
    }

    impl MockDockerManifestGenerator {
        pub fn new() -> Self {
            Self {
                should_fail: false,
                generated_manifests: Mutex::new(Vec::new()),
            }
        }

        pub fn with_failure() -> Self {
            Self {
                should_fail: true,
                generated_manifests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl DockerManifestGenerator for MockDockerManifestGenerator {
        async fn generate_manifest(
            &self,
            command: &GenerateDockerManifestCommand,
            layers: Vec<DockerLayerInfo>,
            config: DockerConfigInfo,
        ) -> Result<DockerManifestDto, DockerManifestGenerationError> {
            self.generated_manifests.lock().unwrap().push((command.clone(), layers.clone(), config.clone()));

            if self.should_fail {
                return Err(DockerManifestGenerationError::GenerationFailed {
                    reason: "Mock generation failure".to_string(),
                });
            }

            // Generar un manifest mock V2.2
            Ok(DockerManifestDto::V2_2(crate::features::generate_docker_manifest::dto::DockerManifestV2_2 {
                schema_version: 2,
                media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
                layers: layers.into_iter().map(|layer| {
                    crate::features::generate_docker_manifest::dto::Layer {
                        media_type: layer.media_type,
                        size: layer.size,
                        digest: layer.digest,
                        urls: layer.urls,
                    }
                }).collect(),
                config: crate::features::generate_docker_manifest::dto::Config {
                    media_type: config.media_type,
                    size: config.size,
                    digest: config.digest,
                },
            }))
        }
    }

    /// Mock para DockerLayerLister
    pub struct MockDockerLayerLister {
        pub layers: Mutex<HashMap<String, Vec<DockerLayerInfo>>>,
        pub configs: Mutex<HashMap<String, DockerConfigInfo>>,
        pub should_fail: bool,
    }

    impl MockDockerLayerLister {
        pub fn new() -> Self {
            Self {
                layers: Mutex::new(HashMap::new()),
                configs: Mutex::new(HashMap::new()),
                should_fail: false,
            }
        }

        pub fn with_layers_and_config(
            repository: &str,
            tag: &str,
            layers: Vec<DockerLayerInfo>,
            config: DockerConfigInfo,
        ) -> Self {
            let key = format!("{}:{}", repository, tag);
            let mut layers_map = HashMap::new();
            layers_map.insert(key.clone(), layers);
            
            let mut configs_map = HashMap::new();
            configs_map.insert(key, config);

            Self {
                layers: Mutex::new(layers_map),
                configs: Mutex::new(configs_map),
                should_fail: false,
            }
        }

        pub fn with_failure() -> Self {
            Self {
                layers: Mutex::new(HashMap::new()),
                configs: Mutex::new(HashMap::new()),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl DockerLayerLister for MockDockerLayerLister {
        async fn list_layers(
            &self,
            repository_name: &str,
            tag: &str,
        ) -> Result<Vec<DockerLayerInfo>, DockerLayerListingError> {
            if self.should_fail {
                return Err(DockerLayerListingError::Internal("Mock failure".to_string()));
            }

            let key = format!("{}:{}", repository_name, tag);
            self.layers.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerLayerListingError::NoLayersFound {
                    repository: repository_name.to_string(),
                    tag: tag.to_string(),
                })
        }

        async fn get_config(
            &self,
            repository_name: &str,
            tag: &str,
        ) -> Result<DockerConfigInfo, DockerConfigRetrievalError> {
            if self.should_fail {
                return Err(DockerConfigRetrievalError::Internal("Mock failure".to_string()));
            }

            let key = format!("{}:{}", repository_name, tag);
            self.configs.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerConfigRetrievalError::ConfigNotFound {
                    repository: repository_name.to_string(),
                    tag: tag.to_string(),
                })
        }
    }

    /// Mock para DockerManifestCache
    pub struct MockDockerManifestCache {
        pub cache: Mutex<HashMap<String, CachedDockerManifest>>,
        pub should_fail: bool,
    }

    impl MockDockerManifestCache {
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
                should_fail: false,
            }
        }

        pub fn with_cached_manifest(
            repository: &str,
            tag: &str,
            media_type: &str,
            manifest: DockerManifestDto,
            digest: &str,
        ) -> Self {
            let key = format!("{}:{}:{}", repository, tag, media_type);
            let cached = CachedDockerManifest {
                manifest,
                digest: digest.to_string(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                media_type: media_type.to_string(),
            };

            let mut cache = HashMap::new();
            cache.insert(key, cached);

            Self {
                cache: Mutex::new(cache),
                should_fail: false,
            }
        }

        pub fn with_failure() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl DockerManifestCache for MockDockerManifestCache {
        async fn get_cached_manifest(
            &self,
            repository_name: &str,
            tag: &str,
            media_type: &str,
        ) -> Result<Option<CachedDockerManifest>, DockerCacheError> {
            if self.should_fail {
                return Err(DockerCacheError::CacheUnavailable);
            }

            let key = format!("{}:{}:{}", repository_name, tag, media_type);
            Ok(self.cache.lock().unwrap().get(&key).cloned())
        }

        async fn cache_manifest(
            &self,
            repository_name: &str,
            tag: &str,
            media_type: &str,
            manifest: &DockerManifestDto,
            digest: &str,
        ) -> Result<(), DockerCacheError> {
            if self.should_fail {
                return Err(DockerCacheError::CacheUnavailable);
            }

            let key = format!("{}:{}:{}", repository_name, tag, media_type);
            let cached = CachedDockerManifest {
                manifest: manifest.clone(),
                digest: digest.to_string(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                media_type: media_type.to_string(),
            };

            self.cache.lock().unwrap().insert(key, cached);
            Ok(())
        }

        async fn invalidate_cache(
            &self,
            repository_name: &str,
            tag: &str,
        ) -> Result<(), DockerCacheError> {
            if self.should_fail {
                return Err(DockerCacheError::CacheUnavailable);
            }

            let prefix = format!("{}:{}:", repository_name, tag);
            self.cache.lock().unwrap().retain(|key, _| !key.starts_with(&prefix));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test::*;

    #[tokio::test]
    async fn test_mock_docker_manifest_generator_success() {
        let generator = MockDockerManifestGenerator::new();
        let command = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let layers = vec![
            DockerLayerInfo {
                digest: "sha256:layer1".to_string(),
                size: 1000,
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                urls: None,
            },
        ];

        let config = DockerConfigInfo {
            digest: "sha256:config1".to_string(),
            size: 500,
            media_type: "application/vnd.docker.container.image.v1+json".to_string(),
            content: b"{}".to_vec(),
        };

        let result = generator.generate_manifest(&command, layers.clone(), config.clone()).await;
        assert!(result.is_ok());

        let manifests = generator.generated_manifests.lock().unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].0.repository_name, "test/repo");
    }

    #[tokio::test]
    async fn test_mock_docker_manifest_generator_failure() {
        let generator = MockDockerManifestGenerator::with_failure();
        let command = GenerateDockerManifestCommand {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            repository_id: "test-repo".to_string(),
            media_type: None,
            force_regenerate: false,
        };

        let layers = vec![];
        let config = DockerConfigInfo {
            digest: "sha256:config1".to_string(),
            size: 500,
            media_type: "application/json".to_string(),
            content: b"{}".to_vec(),
        };

        let result = generator.generate_manifest(&command, layers, config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_docker_layer_lister_success() {
        let layers = vec![
            DockerLayerInfo {
                digest: "sha256:layer1".to_string(),
                size: 1000,
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                urls: None,
            },
        ];

        let config = DockerConfigInfo {
            digest: "sha256:config1".to_string(),
            size: 500,
            media_type: "application/vnd.docker.container.image.v1+json".to_string(),
            content: b"{}".to_vec(),
        };

        let lister = MockDockerLayerLister::with_layers_and_config("test/repo", "latest", layers.clone(), config.clone());

        let listed_layers = lister.list_layers("test/repo", "latest").await.unwrap();
        assert_eq!(listed_layers.len(), 1);
        assert_eq!(listed_layers[0].digest, "sha256:layer1");

        let retrieved_config = lister.get_config("test/repo", "latest").await.unwrap();
        assert_eq!(retrieved_config.digest, "sha256:config1");
    }

    #[tokio::test]
    async fn test_mock_docker_layer_lister_not_found() {
        let lister = MockDockerLayerLister::new();

        let layers_result = lister.list_layers("test/repo", "latest").await;
        assert!(matches!(layers_result, Err(DockerLayerListingError::NoLayersFound { .. })));

        let config_result = lister.get_config("test/repo", "latest").await;
        assert!(matches!(config_result, Err(DockerConfigRetrievalError::ConfigNotFound { .. })));
    }

    #[tokio::test]
    async fn test_mock_docker_manifest_cache() {
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

        let cache = MockDockerManifestCache::with_cached_manifest(
            "test/repo",
            "latest",
            "application/vnd.docker.distribution.manifest.v2+json",
            manifest.clone(),
            "sha256:manifest123",
        );

        let cached = cache.get_cached_manifest("test/repo", "latest", "application/vnd.docker.distribution.manifest.v2+json").await.unwrap();
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().digest, "sha256:manifest123");

        // Test cache miss
        let not_cached = cache.get_cached_manifest("test/repo", "latest", "application/vnd.oci.image.manifest.v1+json").await.unwrap();
        assert!(not_cached.is_none());
    }
}