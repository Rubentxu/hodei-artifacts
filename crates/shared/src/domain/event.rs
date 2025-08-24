//! Eventos de dominio genéricos compartidos entre bounded contexts cuando sea seguro.
//! Eventos específicos deben vivir en el crate dueño del agregado.
use serde::{Serialize, Deserialize};
use crate::domain::model::{ArtifactId, RepositoryId, UserId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactUploadedEvent {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub uploaded_by: UserId,
    pub occurred_at: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDownloadRequestedEvent {
    pub artifact_id: ArtifactId,
    pub requested_by: UserId,
    pub occurred_at: IsoTimestamp,
}

