
// crates/distribution/src/features/generate_docker_manifest/adapter.rs

//! Adaptadores de infraestructura para la generación de manifests Docker

use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, warn, error, instrument};
use serde_json;
use sha2::{Sha256, Digest};

use crate::features::generate_docker_manifest::ports::{
    DockerManifestGenerator, DockerLayerLister, DockerManifestCache,
    DockerManifestGenerationError, DockerLayerListingError, DockerConfigRetrievalError, DockerCacheError,
    DockerLayerInfo, DockerConfigInfo, CachedDockerManifest,
};
use crate::features::generate_docker_manifest::dto::{
    DockerManifestDto, DockerManifestV2_2, DockerManifestListV2_1, Config, DockerDescriptor,
};

/// Adaptador S3 para generar manifests Docker
pub struct S3DockerManifestGenerator {
    s3_client: Arc<dyn S3Client>,
    bucket_name: String,
}

impl S3DockerManifestGenerator {
    pub fn new(s3_client: Arc<dyn S3Client>, bucket_name: String) -> Self {
        Self { s3_client, bucket_name }
    }
}

#[async_trait]
impl DockerManifestGenerator for S3DockerManifestGenerator {
    #[instrument(skip(self, layers, config))]
    async fn generate_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        layers: Vec<DockerLayerInfo>,
        config: DockerConfigInfo,
        media_type: &str,
    ) -> Result<DockerManifestDto, DockerManifestGenerationError> {
        info!(
            repository = repository_name,
            tag = tag,
            layer_count = layers.len(),
            config_digest = %config.digest,
            media_type = media_type,
            "Generating Docker manifest in S3"
        );

        match media_type {
            "application/vnd.docker.distribution.manifest.v2+json" => {
                self.generate_v2_2_manifest(repository_name, tag, layers, config).await
            }
            "application/vnd.docker.distribution.manifest.list.v2+json" => {
                self.generate_manifest_list(repository_name, tag, layers, config).await
            }
            "application/vnd.oci.image.manifest.v1+json" => {
                self.generate_oci_manifest(repository_name, tag, layers, config).await
            }
            _ => {
                error!(
                    repository = repository_name,
                    tag = tag,
                    media_type = media_type,
                    "Unsupported media type for Docker manifest generation"
                );
                Err(DockerManifestGenerationError::UnsupportedMediaType {
                    media_type: media_type.to_string(),
                })
            }
        }
    }
}

impl S3DockerManifestGenerator {
    /// Generar manifest Docker V2.2
    #[instrument(skip(self, layers, config))]
    async fn generate_v2_2_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        layers: Vec<DockerLayerInfo>,
        config: DockerConfigInfo,
    ) -> Result<DockerManifestDto, DockerManifestGenerationError> {
        let manifest = DockerManifestV2_2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            config: Config {
                media_type: config.media_type,
                size: config.size,
                digest: config.digest,
            },
            layers: layers.into_iter().map(|layer| DockerDescriptor {
                media_type: layer.media_type,
                size: layer.size,
                digest: layer.digest,
            }).collect(),
        };

        Ok(DockerManifestDto::V2_2(manifest))
    }

    /// Generar manifest list Docker V2.1
    #[instrument(skip(self, layers, config))]
    async fn generate_manifest_list(
        &self,
        repository_name: &str,
        tag: &str,
        layers: Vec<DockerLayerInfo>,
        config: DockerConfigInfo,
    ) -> Result<DockerManifestDto, DockerManifestGenerationError> {
        let manifest_list = DockerManifestListV2_1 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.list.v2+json".to_string(),
            manifests: vec![], // En una implementación real, esto contendría múltiples manifests
        };

        Ok(DockerManifestDto::ManifestList(manifest_list))
    }

    /// Generar manifest OCI
    #[instrument(skip(self, layers, config))]
    async fn generate_oci_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        layers: Vec<DockerLayerInfo>,
        config: DockerConfigInfo,
    ) -> Result<DockerManifestDto, DockerManifestGenerationError> {
        // OCI manifests son similares a Docker V2.2 pero con diferentes media types
        let manifest = DockerManifestV2_2 {
            schema_version: 2,
            media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
            config: Config {
                media_type: "application/vnd.oci.image.config.v1+json".to_string(),
                size: config.size,
                digest: config.digest,
            },
            layers: layers.into_iter().map(|layer| DockerDescriptor {
                media_type: if layer.media_type.contains("docker") {
                    layer.media_type.replace("docker", "oci")
                } else {
                    "application/vnd.oci.image.layer.v1.tar+gzip".to_string()
                },
                size: layer.size,
                digest: layer.digest,
            }).collect(),
        };

        Ok(DockerManifestDto::V2_2(manifest))
    }
}

/// Adaptador S3 para listar capas Docker
pub struct S3DockerLayerLister {
    s3_client: Arc<dyn S3Client>,
    bucket_name: String,
}

impl S3DockerLayerLister {
    pub fn new(s3_client: Arc<dyn S3Client>, bucket_name: String) -> Self {
        Self { s3_client, bucket_name }
    }
}

#[async_trait]
impl DockerLayerLister for S3DockerLayerLister {
    #[instrument(skip(self))]
    async fn list_layers(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<Vec<DockerLayerInfo>, DockerLayerListingError> {
        info!(
            repository = repository_name,
            tag = tag,
            "Listing Docker layers from S3"
        );

        let prefix = format!("docker/{}/{}/layers/", repository_name, tag);
        
        match self.s3_client.list_objects(&self.bucket_name, &prefix).await {
            Ok(objects) => {
                let mut layers = Vec::new();
                
                for object in objects {
                    if object.key.ends_with(".tar.gz") || object.key.ends_with(".tar") {
                        let layer_info = DockerLayerInfo {
                            digest: self.extract_digest_from_key(&object.key),
                            size: object.size as i64,
                            media_type: self.determine_layer_media_type(&object.key),
                        };
                        layers.push(layer_info);
                    }
                }

                if layers.is_empty() {
                    warn!(
                        repository = repository_name,
                        tag = tag,
                        "No layers found in S3"
                    );
                    return Err(DockerLayerListingError::NoLayersFound {
                        repository: repository_name.to_string(),
                        tag: tag.to_string(),
                    });
                }

                info!(
                    repository = repository_name,
                    tag = tag,
                    layer_count = layers.len(),
                    "Successfully listed Docker layers"
                );

                Ok(layers)
            }
            Err(e) => {
                error!(
                    repository = repository_name,
                    tag = tag,
                    error = %e,
                    "Failed to list Docker layers from S3"
                );
                Err(DockerLayerListingError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }

    #[instrument(skip(self))]
    async fn get_config(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<DockerConfigInfo, DockerConfigRetrievalError> {
        info!(
            repository = repository_name,
            tag = tag,
            "Retrieving Docker config from S3"
        );

        let config_key = format!("docker/{}/{}/config.json", repository_name, tag);
        
        match self.s3_client.get_object(&self.bucket_name, &config_key).await {
            Ok(data) => {
                // Parsear el config.json
                let config: serde_json::Value = serde_json::from_slice(&data)
                    .map_err(|e| DockerConfigRetrievalError::InvalidConfig {
                        repository: repository_name.to_string(),
                        tag: tag.to_string(),
                        error: e.to_string(),
                    })?;

                let config_info = DockerConfigInfo {
                    digest: self.calculate_config_digest(&data),
                    size: data.len() as i64,
                    media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                };

                info!(
                    repository = repository_name,
                    tag = tag,
                    config_digest = %config_info.digest,
                    "Successfully retrieved Docker config"
                );

                Ok(config_info)
            }
            Err(e) => {
                error!(
                    repository = repository_name,
                    tag = tag,
                    error = %e,
                    "Failed to retrieve Docker config from S3"
                );
                Err(DockerConfigRetrievalError::StorageError(format!("S3 error: {}", e)))
            }
        }
    }
}

impl S3DockerLayerLister {
    /// Extraer el digest de la clave S3
    fn extract_digest_from_key(&self, key: &str) -> String {
        // Las claves S3 para capas Docker típicamente contienen el digest
        // Por ejemplo: docker/repo/tag/layers/sha256:abc123...
        if let Some(digest_part) = key.split('/').last() {
            if digest_part.starts_with("sha256:") {
                return digest_part.to_string();
            }
        }
        
        // Si no podemos extraer el digest, generar uno basado en la clave
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Determinar el tipo de media basado en la extensión del archivo
    fn determine_layer_media_type(&self, key: &str) -> String {
        if key.ends_with(".tar.gz") {
            "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string()
        } else if key.ends_with(".tar") {
            "application/vnd.docker.image.rootfs.diff.tar".to_string()
        } else {
            "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string()
        }
    }

    /// Calcular el digest del config
    fn calculate_config_digest(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("sha256:{:x}", hasher.finalize())
    }
}

/// Adaptador Redis para caché de manifests Docker
pub struct RedisDockerManifestCache {
    redis_client: Arc<dyn RedisClient>,
    ttl_seconds: u64,
}

impl RedisDockerManifestCache {
    pub fn new(redis_client: Arc<dyn RedisClient>, ttl_seconds: u64) -> Self {
        Self { redis_client, ttl_seconds }
    }

    /// Generar la clave de caché para un manifest
    fn generate_cache_key(&self, repository_name: &str, tag: &str, media_type: &str) -> String {
        format!("docker:manifest:{}:{}:{}", repository_name, tag, media_type)
    }
}

#[async_trait]
impl DockerManifestCache for RedisDockerManifestCache {
    #[instrument(skip(self))]
    async fn get_cached_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        media_type: &str,
    ) -> Result<Option<CachedDockerManifest>, DockerCacheError> {
        let cache_key = self.generate_cache_key(repository_name, tag, media_type);
        
        match self.redis_client.get(&cache_key).await {
            Ok(Some(data)) => {
                match serde_json::from_slice::<CachedDockerManifest>(&data) {
                    Ok(cached) => {
                        info!(
                            repository = repository_name,
                            tag = tag,
                            media_type = media_type,
                            "Found cached Docker manifest"
                        );
                        Ok(Some(cached))
                    }
                    Err(e) => {
                        warn!(
                            repository = repository_name,
                            tag = tag,
                            media_type = media_type,
                            error = %e,
                            "Failed to deserialize cached Docker manifest"
                        );
                        Ok(None)
                    }
                }
            }
            Ok(None) => {
                info!(
                    repository = repository_name,
                    tag = tag,
                    media_type = media_type,
                    "No cached Docker manifest found"
                );
                Ok(None)
            }
            Err(e) => {
                error!(
                    repository = repository_name,
                    tag = tag,
                    media_type = media_type,
                    error = %e,
                    "Failed to get cached Docker manifest from Redis"
                );
                Err(DockerCacheError::CacheAccessError(format!("Redis error: {}", e)))
            }
        }
    }

    #[instrument(skip(self, manifest))]
    async fn cache_manifest(
        &self,
        repository_name: &str,
        tag: &str,
        manifest: &DockerManifestDto,
        digest: &str,
        media_type: &str,
    ) -> Result<(), DockerCacheError> {
        let cache_key = self.generate_cache_key(repository_name, tag, media_type);
        
        let cached_manifest = CachedDockerManifest {
            manifest: manifest.clone(),
            digest: digest.to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            media_type: media_type.to_string(),
        };

        match serde_json::to_vec(&cached_manifest) {
            Ok(data) => {
                match self.redis_client.set_ex(&cache_key, &data, self.ttl_seconds).await {
                    Ok(_) => {
                        info!(
                            repository = repository_name,
                            tag = tag,
                            media_type = media_type,
                            ttl_seconds = self.ttl_seconds,
                            "Successfully cached Docker manifest"
                        );
                        Ok(())
                    }
                    Err(e) => {
                        error!(
                            repository = repository_name,
                            tag = tag,
                            media_type = media_type,
                            error =