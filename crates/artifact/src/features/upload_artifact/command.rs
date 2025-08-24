use shared::{RepositoryId, UserId};
use crate::domain::model::{ArtifactVersion, ArtifactChecksum};

#[derive(Clone)]
pub struct UploadArtifactCommand {
    pub repository_id: RepositoryId,
    pub version: ArtifactVersion,
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: ArtifactChecksum,
    pub user_id: UserId,
    pub bytes: Vec<u8>,
}

pub struct UploadArtifactResult {
    pub artifact_id: shared::ArtifactId,
}
