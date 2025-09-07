// crates/shared/src/models.rs

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use crate::hrn::PhysicalArtifactId;
use crate::enums::{HashAlgorithm, ArtifactType, ArtifactRole};

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

/// Una referencia tipada desde una `PackageVersion` a un `PhysicalArtifact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    /// El HRN del `PhysicalArtifact` al que se refiere.
    pub artifact_hrn: PhysicalArtifactId,
    /// El tipo de fichero (binario principal, firma, SBOM, etc.).
    pub artifact_type: ArtifactType,
    /// El rol semántico del fichero dentro del paquete (ej. "sources", "javadoc").
    pub role: Option<ArtifactRole>,
}