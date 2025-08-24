use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedField(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSearchDocument {
    pub artifact_id: ArtifactId,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub indexed_at: IsoTimestamp,
}

