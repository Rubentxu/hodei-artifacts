// crates/artifact/src/domain/oci.rs

use shared::models::ContentHash;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Representa un manifiesto de imagen OCI, que es la raíz de una imagen de contenedor.
/// El manifiesto en sí es un `PhysicalArtifact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciManifest {
    /// El digest del manifiesto.
    pub digest: ContentHash,
    /// El tipo de media, ej. "application/vnd.oci.image.manifest.v1+json".
    pub media_type: String,
    /// Descriptor que apunta al fichero de configuración de la imagen.
    pub config: OciDescriptor,
    /// Lista ordenada de descriptores que apuntan a las capas de la imagen.
    pub layers: Vec<OciDescriptor>,
    /// Anotaciones opcionales.
    pub annotations: Option<HashMap<String, String>>,
}

/// Un descriptor OCI, usado para `config` y `layers`. Apunta a otro `PhysicalArtifact`
/// a través de su `digest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciDescriptor {
    pub media_type: String,
    pub digest: ContentHash,
    pub size: u64,
    pub annotations: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciHistory {
    pub created: Option<String>,
    pub author: Option<String>,
    pub created_by: Option<String>,
    pub comment: Option<String>,
    pub empty_layer: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciRootFs {
    pub r#type: String,
    pub diff_ids: Vec<String>,
}

/// Representa el fichero de configuración de una imagen OCI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciImageConfig {
    pub architecture: String,
    pub os: String,
    /// Configuración del contenedor en sí.
    pub config: OciContainerConfig,
    /// Historial de cómo se construyó cada capa.
    pub history: Vec<OciHistory>,
    /// Sistema de ficheros raíz.
    pub rootfs: OciRootFs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciContainerConfig {
    pub user: Option<String>,
    pub exposed_ports: Option<HashMap<String, serde_json::Value>>,
    pub env: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    pub working_dir: Option<String>,
    pub labels: Option<HashMap<String, String>>,
}
// ... otras structs OCI: OciHistory, OciRootFs, etc.
