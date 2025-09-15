use crate::domain::error;
use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, OrganizationId, RepositoryId};
use shared::lifecycle::Lifecycle;
pub(crate) use shared::models::{ArtifactReference, PackageCoordinates};
use std::collections::HashMap;
use time::OffsetDateTime;

/// La representación de una única versión de un paquete publicado en un repositorio.
/// Es el Agregado Raíz principal de este Bounded Context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion {
    /// El HRN único y global de esta versión del paquete.
    /// Formato: `hrn:hodei:artifact:<region>:<org_id>:package-version/<repo_name>/<path>`
    pub hrn: Hrn,

    /// HRN de la organización propietaria (denormalizado para búsquedas y políticas rápidas).
    pub organization_hrn: OrganizationId,

    /// HRN del repositorio que contiene esta versión del paquete.
    pub repository_hrn: RepositoryId,

    /// Las coordenadas que identifican unívocamente al paquete en su ecosistema.
    pub coordinates: PackageCoordinates,

    /// El estado actual del ciclo de vida del artefacto.
    pub status: ArtifactStatus,

    /// Metadatos descriptivos y de uso.
    pub metadata: PackageMetadata,

    /// Lista de referencias a los ficheros físicos que componen este paquete.
    pub artifacts: Vec<ArtifactReference>,

    /// Lista de las dependencias directas de este paquete.
    pub dependencies: Vec<ArtifactDependency>,

    /// Etiquetas de texto libre para clasificación y búsqueda.
    pub tags: Vec<String>,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,

    /// Si este artefacto es de tipo OCI, HRN al `PhysicalArtifact` que contiene el manifiesto.
    pub oci_manifest_hrn: Option<Hrn>,
}

impl PackageVersion {
    /// Pone en cuarentena un artefacto si está en un estado válido.
    /// Este método encapsula la lógica de negocio para la transición de estado.
    pub fn quarantine(
        &mut self,
        _reason: String,
        _by: Hrn,
        _at: OffsetDateTime,
    ) -> Result<(), error::DomainError> {
        // ... Lógica para una transición de estado segura
        Ok(())
    }
    // ... otros métodos de negocio (deprecate, ban, etc.)
}

/// Metadatos detallados de una `PackageVersion`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub description: Option<String>,
    pub licenses: Vec<String>, // SPDX license identifiers
    pub authors: Vec<String>,
    pub project_url: Option<String>,
    pub repository_url: Option<String>,
    pub last_downloaded_at: Option<OffsetDateTime>,
    pub download_count: u64,
    /// Metadatos personalizados para extensibilidad.
    pub custom_properties: HashMap<String, String>,
}

/// Una dependencia de software de este `PackageVersion`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDependency {
    pub coordinates: PackageCoordinates,
    pub scope: String,              // "compile", "runtime", "test", etc.
    pub version_constraint: String, // ej. "^1.2.3", "~4.5.0"
    pub is_optional: bool,
}

/// El estado del artefacto, con datos contextuales.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArtifactStatus {
    Active,
    Quarantined {
        reason: String,
        since: OffsetDateTime,
    },
    Banned {
        reason: String,
        since: OffsetDateTime,
    },
    Deprecated {
        successor_hrn: Option<Hrn>,
    },
}
