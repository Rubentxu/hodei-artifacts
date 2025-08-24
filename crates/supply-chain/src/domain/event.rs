use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp};
use crate::domain::model::{SbomId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomGenerationStarted {
    pub artifact_id: ArtifactId,
    pub sbom_id: SbomId,
    pub occurred_at: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomGenerated {
    pub artifact_id: ArtifactId,
    pub sbom_id: SbomId,
    pub component_count: u32,
    pub occurred_at: IsoTimestamp,
}

