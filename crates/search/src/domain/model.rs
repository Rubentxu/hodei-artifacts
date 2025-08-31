use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp, RepositoryId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedField(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSearchDocument {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub indexed_at: IsoTimestamp,
    pub relevance_score: f64,
}
