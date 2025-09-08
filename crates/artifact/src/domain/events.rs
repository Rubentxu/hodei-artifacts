// crates/artifact/src/domain/events.rs

use shared::hrn::{Hrn, RepositoryId, UserId};
use shared::models::{PackageCoordinates, ArtifactReference};
use crate::domain::package_version::ArtifactStatus;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `artifact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactEvent {
  /// Se ha publicado una nueva versión de un paquete. Este es el evento principal.
  PackageVersionPublished(PackageVersionPublished),
  /// Se ha eliminado una versión de un paquete.
  PackageVersionDeleted(PackageVersionDeleted),
  /// El estado de una versión de paquete ha cambiado (ej. a Quarantined).
  PackageVersionStatusChanged(PackageVersionStatusChanged),
  /// Progreso de subida de un artefacto reanudable (emitido periódicamente o por chunk).
  UploadProgressUpdated {
      upload_id: String,
      progress: u64,
      bytes_uploaded: u64,
      total_bytes: u64,
  },
  /// Se ha completado la subida de un artefacto
  ArtifactUploaded {
      artifact: ArtifactReference,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersionPublished {
  pub hrn: Hrn,
  pub repository_hrn: RepositoryId,
  pub coordinates: PackageCoordinates,
  /// Lista de todos los ficheros físicos que componen este paquete.
  pub artifacts: Vec<ArtifactReference>,
  pub publisher_hrn: UserId,
  pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersionDeleted {
  pub hrn: Hrn,
  pub deleted_by: Hrn,
  pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersionStatusChanged {
  pub hrn: Hrn,
  pub old_status: ArtifactStatus,
  pub new_status: ArtifactStatus,
  pub changed_by: Hrn,
  pub at: OffsetDateTime,
}

