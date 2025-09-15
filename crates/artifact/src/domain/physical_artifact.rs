// crates/artifact/src/domain/physical_artifact.rs

use serde::{Deserialize, Serialize};
use shared::enums::HashAlgorithm;
use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use shared::models::ContentHash;
use std::collections::HashMap;

/// Representa un fichero físico, inmutable, almacenado en el backend de almacenamiento.
/// Su identidad es su `content_hash`. La misma instancia puede ser referenciada
/// por múltiples `PackageVersion`, permitiendo la deduplicación.
/// Es un Agregado Raíz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalArtifact {
    /// HRN único para este blob físico.
    /// Formato: `hrn:hodei:artifact:<region>:<org_id>:physical-artifact/<hash_alg>-<hash_value>`
    pub hrn: Hrn,

    /// Organización que subió originalmente este artefacto.
    pub organization_hrn: OrganizationId,

    /// El hash del contenido, que actúa como su identificador único.
    pub content_hash: ContentHash,

    /// Tamaño del fichero en bytes.
    pub size_in_bytes: u64,

    /// Mapa de otros checksums calculados para el fichero (ej. MD5, SHA-1).
    pub checksums: HashMap<HashAlgorithm, String>,

    /// Ubicación en el backend de almacenamiento (ej. `s3://my-bucket/cas/sha256/abc...`).
    pub storage_location: String,

    /// Tipo MIME detectado del contenido (ej. `application/java-archive`, `application/octet-stream`).
    pub mime_type: String,

    /// Información de auditoría y ciclo de vida (útil para la recolección de basura).
    pub lifecycle: Lifecycle,
}
