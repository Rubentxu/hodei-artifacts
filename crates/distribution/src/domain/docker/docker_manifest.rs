// crates/distribution/src/domain/docker/docker_manifest.rs

use serde::{Serialize, Deserialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use crate::domain::error::{FormatError, DistributionResult};
use std::collections::HashMap;

/// Docker Manifest V2 Schema 2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestV2 {
    pub schema_version: u32,
    pub media_type: String,
    pub config: ConfigDescriptor,
    pub layers: Vec<LayerDescriptor>,
}

/// Descriptor de configuración
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDescriptor {
    pub media_type: String,
    pub size: u64,
    pub digest: String,
}

/// Descriptor de capa
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerDescriptor {
    pub media_type: String,
    pub size: u64,
    pub digest: String,
}

/// Docker Manifest List (multi-arquitectura)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestList {
    pub schema_version: u32,
    pub media_type: String,
    pub manifests: Vec<ManifestDescriptor>,
}

/// Descriptor de manifest en una lista
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDescriptor {
    pub media_type: String,
    pub size: u64,
    pub digest: String,
    pub platform: Option<Platform>,
}

/// Información de plataforma
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub architecture: String,
    pub os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_features: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
}

/// Configuración de imagen Docker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub architecture: String,
    pub config: Config,
    pub rootfs: RootFS,
    pub history: Vec<History>,
}

/// Configuración de contenedor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub hostname: Option<String>,
    pub domainname: Option<String>,
    pub user: Option<String>,
    pub attach_stdin: Option<bool>,
    pub attach_stdout: Option<bool>,
    pub attach_stderr: Option<bool>,
    pub exposed_ports: Option<HashMap<String, serde_json::Value>>,
    pub tty: Option<bool>,
    pub open_stdin: Option<bool>,
    pub stdin_once: Option<bool>,
    pub env: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    pub image: Option<String>,
    pub volumes: Option<HashMap<String, serde_json::Value>>,
    pub working_dir: Option<String>,
    pub entrypoint: Option<serde_json::Value>,
    pub on_build: Option<Vec<String>>,
    pub labels: Option<HashMap<String, String>>,
    pub stop_signal: Option<String>,
}

/// Sistema de archivos raíz
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFS {
    #[serde(rename = "type")]
    pub rootfs_type: String,
    pub diff_ids: Vec<String>,
}

/// Historial de capas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    pub created: Option<String>,
    pub created_by: Option<String>,
    pub author: Option<String>,
    pub comment: Option<String>,
    pub empty_layer: Option<bool>,
}

/// Información de una imagen Docker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerImageInfo {
    pub id: String,
    pub parent: Option<String>,
    pub comment: Option<String>,
    pub created: String,
    pub container: Option<String>,
    pub container_config: Option<Config>,
    pub docker_version: Option<String>,
    pub author: Option<String>,
    pub config: Option<Config>,
    pub architecture: String,
    pub os: String,
    pub size: Option<u64>,
    pub virtual_size: Option<u64>,
}

/// Generador de manifests Docker
pub struct DockerManifestGenerator;

impl DockerManifestGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Genera un Docker Manifest V2 Schema 2
    pub fn generate_manifest_v2(
        &self,
        config_digest: &str,
        config_size: u64,
        layers: Vec<(String, u64)>, // (digest, size)
    ) -> DistributionResult<DockerManifestV2> {
        let config = ConfigDescriptor {
            media_type: "application/vnd.docker.container.image.v1+json".to_string(),
            size: config_size,
            digest: config_digest.to_string(),
        };

        let layers = layers.into_iter()
            .map(|(digest, size)| LayerDescriptor {
                media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                size,
                digest,
            })
            .collect();

        Ok(DockerManifestV2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            config,
            layers,
        })
    }

    /// Genera un Docker Manifest List (multi-arquitectura)
    pub fn generate_manifest_list(
        &self,
        manifests: Vec<(String, u64, String, Option<Platform>)>, // (digest, size, media_type, platform)
    ) -> DistributionResult<DockerManifestList> {
        let manifests = manifests.into_iter()
            .map(|(digest, size, media_type, platform)| ManifestDescriptor {
                media_type,
                size,
                digest,
                platform,
            })
            .collect();

        Ok(DockerManifestList {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.list.v2+json".to_string(),
            manifests,
        })
    }

    /// Genera una configuración de imagen Docker
    pub fn generate_config(
        &self,
        architecture: &str,
        os: &str,
        config_data: HashMap<String, serde_json::Value>,
    ) -> DistributionResult<DockerConfig> {
        let config = Config {
            hostname: config_data.get("Hostname").and_then(|v| v.as_str()).map(String::from),
            domainname: config_data.get("Domainname").and_then(|v| v.as_str()).map(String::from),
            user: config_data.get("User").and_then(|v| v.as_str()).map(String::from),
            attach_stdin: config_data.get("AttachStdin").and_then(|v| v.as_bool()),
            attach_stdout: config_data.get("AttachStdout").and_then(|v| v.as_bool()),
            attach_stderr: config_data.get("AttachStderr").and_then(|v| v.as_bool()),
            exposed_ports: config_data.get("ExposedPorts").and_then(|v| v.as_object()).map(|obj| {
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            }),
            tty: config_data.get("Tty").and_then(|v| v.as_bool()),
            open_stdin: config_data.get("OpenStdin").and_then(|v| v.as_bool()),
            stdin_once: config_data.get("StdinOnce").and_then(|v| v.as_bool()),
            env: config_data.get("Env").and_then(|v| v.as_array()).map(|arr| {
                arr.iter().filter_map(|v| v.as_str()).map(String::from).collect()
            }),
            cmd: config_data.get("Cmd").and_then(|v| v.as_array()).map(|arr| {
                arr.iter().filter_map(|v| v.as_str()).map(String::from).collect()
            }),
            image: config_data.get("Image").and_then(|v| v.as_str()).map(String::from),
            volumes: config_data.get("Volumes").and_then(|v| v.as_object()).map(|obj| {
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            }),
            working_dir: config_data.get("WorkingDir").and_then(|v| v.as_str()).map(String::from),
            entrypoint: config_data.get("Entrypoint").cloned(),
            on_build: config_data.get("OnBuild").and_then(|v| v.as_array()).map(|arr| {
                arr.iter().filter_map(|v| v.as_str()).map(String::from).collect()
            }),
            labels: config_data.get("Labels").and_then(|v| v.as_object()).map(|obj| {
                obj.iter().filter_map(|(k, v)| {
                    v.as_str().map(|val| (k.clone(), val.to_string()))
                }).collect()
            }),
            stop_signal: config_data.get("StopSignal").and_then(|v| v.as_str()).map(String::from),
        };

        let rootfs = RootFS {
            rootfs_type: "layers".to_string(),
            diff_ids: vec![], // Se llenaría con los diff_ids reales
        };

        let history = vec![]; // Se llenaría con el historial real

        Ok(DockerConfig {
            architecture: architecture.to_string(),
            config,
            rootfs,
            history,
        })
    }

    /// Serializa un manifest a JSON
    pub fn serialize_manifest_v2(&self, manifest: &DockerManifestV2) -> DistributionResult<String> {
        serde_json::to_string_pretty(manifest)
            .map_err(|e| FormatError::DockerError(format!("Failed to serialize manifest: {}", e)).into())
    }

    /// Serializa un manifest list a JSON
    pub fn serialize_manifest_list(&self, manifest_list: &DockerManifestList) -> DistributionResult<String> {
        serde_json::to_string_pretty(manifest_list)
            .map_err(|e| FormatError::DockerError(format!("Failed to serialize manifest list: {}", e)).into())
    }

    /// Serializa una configuración a JSON
    pub fn serialize_config(&self, config: &DockerConfig) -> DistributionResult<String> {
        serde_json::to_string_pretty(config)
            .map_err(|e| FormatError::DockerError(format!("Failed to serialize config: {}", e)).into())
    }

    /// Parsea un manifest desde JSON
    pub fn parse_manifest_v2(&self, json_content: &str) -> DistributionResult<DockerManifestV2> {
        serde_json::from_str(json_content)
            .map_err(|e| FormatError::DockerError(format!("Failed to parse manifest: {}", e)).into())
    }

    /// Parsea un manifest list desde JSON
    pub fn parse_manifest_list(&self, json_content: &str) -> DistributionResult<DockerManifestList> {
        serde_json::from_str(json_content)
            .map_err(|e| FormatError::DockerError(format!("Failed to parse manifest list: {}", e)).into())
    }

    /// Genera un digest SHA256 para un blob
    pub fn generate_blob_digest(&self, content: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Genera un manifest digest
    pub fn generate_manifest_digest(&self, manifest_json: &str) -> String {
        self.generate_blob_digest(manifest_json.as_bytes())
    }

    /// Crea una plataforma estándar
    pub fn create_platform(&self, architecture: &str, os: &str) -> Platform {
        Platform {
            architecture: architecture.to_string(),
            os: os.to_string(),
            os_version: None,
            os_features: None,
            variant: None,
            features: None,
        }
    }

    /// Crea una plataforma Linux AMD64 estándar
    pub fn create_linux_amd64_platform(&self) -> Platform {
        self.create_platform("amd64", "linux")
    }

    /// Crea una plataforma ARM64 estándar
    pub fn create_linux_arm64_platform(&self) -> Platform {
        self.create_platform("arm64", "linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_manifest_v2() {
        let generator = DockerManifestGenerator::new();
        
        let config_digest = "sha256:1234567890abcdef";
        let config_size = 1234;
        let layers = vec![
            ("sha256:layer1".to_string(), 5678),
            ("sha256:layer2".to_string(), 9012),
        ];
        
        let manifest = generator.generate_manifest_v2(config_digest, config_size, layers).unwrap();
        
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.media_type, "application/vnd.docker.distribution.manifest.v2+json");
        assert_eq!(manifest.config.digest, config_digest);
        assert_eq!(manifest.config.size, config_size);
        assert_eq!(manifest.layers.len(), 2);
        assert_eq!(manifest.layers[0].digest, "sha256:layer1");
        assert_eq!(manifest.layers[1].digest, "sha256:layer2");
    }

    #[test]
    fn test_generate_manifest_list() {
        let generator = DockerManifestGenerator::new();
        
        let manifests = vec![
            ("sha256:manifest1".to_string(), 1000, "application/vnd.docker.distribution.manifest.v2+json".to_string(), Some(generator.create_linux_amd64_platform())),
            ("sha256:manifest2".to_string(), 1200, "application/vnd.docker.distribution.manifest.v2+json".to_string(), Some(generator.create_linux_arm64_platform())),
        ];
        
        let manifest_list = generator.generate_manifest_list(manifests).unwrap();
        
        assert_eq!(manifest_list.schema_version, 2);
        assert_eq!(manifest_list.media_type, "application/vnd.docker.distribution.manifest.list.v2+json");
        assert_eq!(manifest_list.manifests.len(), 2);
        assert_eq!(manifest_list.manifests[0].digest, "sha256:manifest1");
        assert_eq!(manifest_list.manifests[1].digest, "sha256:manifest2");
        assert!(manifest_list.manifests[0].platform.is_some());
        assert!(manifest_list.manifests[1].platform.is_some());
    }

    #[test]
    fn test_serialize_manifest_v2() {
        let generator = DockerManifestGenerator::new();
        
        let manifest = DockerManifestV2 {
            schema_version: 2,
            media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
            config: ConfigDescriptor {
                media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                size: 1234,
                digest: "sha256:config".to_string(),
            },
            layers: vec![
                LayerDescriptor {
                    media_type: "application/vnd.docker.image.rootfs.diff.tar.gzip".to_string(),
                    size: 5678,
                    digest: "sha256:layer1".to_string(),
                }
            ],
        };
        
        let json = generator.serialize_manifest_v2(&manifest).unwrap();
        
        assert!(json.contains("\"schemaVersion\": 2"));
        assert!(json.contains("\"mediaType\": \"application/vnd.docker.distribution.manifest.v2+json\""));
        assert!(json.contains("\"digest\": \"sha256:config\""));
        assert!(json.contains("\"digest\": \"sha256:layer1\""));
    }

    #[test]
    fn test_parse_manifest_v2() {
        let generator = DockerManifestGenerator::new();
        
        let json = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": 1234,
                "digest": "sha256:config"
            },
            "layers": [
                {
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "size": 5678,
                    "digest": "sha256:layer1"
                }
            ]
        }"#;
        
        let manifest = generator.parse_manifest_v2(json).unwrap();
        
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.config.digest, "sha256:config");
        assert_eq!(manifest.layers.len(), 1);
        assert_eq!(manifest.layers[0].digest, "sha256:layer1");
    }

    #[test]
    fn test_generate_blob_digest() {
        let generator = DockerManifestGenerator::new();
        
        let content = b"Hello, Docker!";
        let digest = generator.generate_blob_digest(content);
        
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71); // "sha256:" + 64 caracteres hex
    }

    #[test]
    fn test_create_platforms() {
        let generator = DockerManifestGenerator::new();
        
        let linux_amd64 = generator.create_linux_amd64_platform();
        assert_eq!(linux_amd64.architecture, "amd64");
        assert_eq!(linux_amd64.os, "linux");
        
        let linux_arm64 = generator.create_linux_arm64_platform();
        assert_eq!(linux_arm64.architecture, "arm64");
        assert_eq!(linux_arm64.os, "linux");
    }
}