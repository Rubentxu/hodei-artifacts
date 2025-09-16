// crates/artifact/src/domain/artifact.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{ArtifactId, RepositoryId, UserId, PhysicalArtifactId};
use time::OffsetDateTime;

use super::package_version::{ArtifactDependency, PackageCoordinates};

/// Agregado lógico `Artifact` que representa la entidad publicada en un repositorio,
/// alineado con el diagrama de dominio. Agrupa metadatos y referencia al artefacto físico.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub repository_id: RepositoryId,
    pub coordinates: ArtifactCoordinates,
    pub metadata: ArtifactMetadata,
    pub uploader_user_id: UserId,
    /// Tipo de artefacto por ecosistema (Maven, Npm, Docker...).
    pub artifact_type: shared::enums::Ecosystem,
    pub physical_artifact_id: PhysicalArtifactId,
    pub created_at: OffsetDateTime,
}

/// Alias para mantener consistencia con el diagrama, reutilizando el VO existente.
pub type ArtifactCoordinates = PackageCoordinates;

/// Metadatos genéricos del artefacto, alineados con el diagrama.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub licenses: Vec<String>,
    pub dependencies: Vec<ArtifactDependency>,
    pub custom_properties: std::collections::HashMap<String, String>,
}
