use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactIndexed {
    pub artifact_id: ArtifactId,
    pub indexed_at: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactIndexFailed {
    pub artifact_id: ArtifactId,
    pub error: String,
    pub occurred_at: IsoTimestamp,
}

