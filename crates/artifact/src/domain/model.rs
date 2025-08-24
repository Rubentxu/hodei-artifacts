use serde::{Serialize, Deserialize};
use shared::{ArtifactId, RepositoryId, IsoTimestamp, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactVersion(pub String);

impl ArtifactVersion { pub fn new(raw: impl Into<String>) -> Self { Self(raw.into()) } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactChecksum(pub String); // sha256
impl ArtifactChecksum { pub fn new(v: impl Into<String>) -> Self { Self(v.into()) } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub repository_id: RepositoryId,
    pub version: ArtifactVersion,
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: ArtifactChecksum,
    pub created_at: IsoTimestamp,
    pub created_by: UserId,
}

impl Artifact {
    pub fn new(repository_id: RepositoryId, version: ArtifactVersion, file_name: String, size_bytes: u64, checksum: ArtifactChecksum, created_by: UserId) -> Self {
        Self { id: ArtifactId::new(), repository_id, version, file_name, size_bytes, checksum, created_at: IsoTimestamp::now(), created_by }
    }
}

