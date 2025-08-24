//! Modelos de dominio Supply Chain (placeholder inicial)
use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomSummary {
    pub sbom_id: SbomId,
    pub artifact_id: ArtifactId,
    pub generated_at: IsoTimestamp,
    pub component_count: u32,
}

