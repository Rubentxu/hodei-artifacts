// crates/shared/src/models.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::enums::HashAlgorithm;

/// El hash criptográfico del contenido de un fichero físico. Es inmutable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash {
    /// El algoritmo utilizado para generar el hash (ej. Sha256).
    pub algorithm: HashAlgorithm,
    /// El valor del hash en formato hexadecimal.
    pub value: String,
}

/// Coordenadas universales que identifican un paquete en cualquier ecosistema.
/// No contiene el ecosistema, ya que este se infiere del `Repository` contenedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// El espacio de nombres del paquete (ej. `@scope` en npm, `groupId` en Maven).
    pub namespace: Option<String>,
    /// El nombre del paquete (ej. `react`, `log4j-core`).
    pub name: String,
    /// La versión del paquete (ej. "18.2.0", "2.17.1").
    pub version: String,
    /// Pares clave-valor para metadatos específicos del ecosistema que son necesarios para la identificación
    /// (ej. `classifier="sources"` en Maven, `os="linux"` en OCI).
    pub qualifiers: HashMap<String, String>,
}

impl PackageCoordinates {
    pub fn new(namespace: &str, name: &str, version: &str) -> Self {
        Self::with_qualifiers(namespace, name, version, HashMap::new())
    }

    pub fn with_qualifiers(
        namespace: &str,
        name: &str,
        version: &str,
        qualifiers: HashMap<String, String>,
    ) -> Self {
        Self {
            namespace: if namespace.is_empty() {
                None
            } else {
                Some(namespace.to_string())
            },
            name: name.to_string(),
            version: version.to_string(),
            qualifiers,
        }
    }
}

/// Referencia a un artefacto físico, alineada con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    /// HRN del artefacto físico.
    pub physical_artifact_hrn: String,
    /// Tamaño del artefacto en bytes.
    pub size_in_bytes: u64,
    /// Hash del contenido del artefacto.
    pub content_hash: ContentHash,
}
