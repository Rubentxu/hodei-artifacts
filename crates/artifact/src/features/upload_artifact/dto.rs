use serde::{Deserialize, Serialize};
use shared::enums::HashAlgorithm;
use shared::models::PackageCoordinates;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UploadArtifactMetadata {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub checksum: Option<String>,
    pub checksum_algorithm: Option<HashAlgorithm>,
}

#[derive(Debug, Clone)]
pub struct UploadArtifactCommand {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub content_length: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UploadArtifactResponse {
    pub hrn: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadArtifactChunkCommand {
    pub upload_id: String,
    pub chunk_number: usize,
    pub total_chunks: usize,
    pub file_name: String,
    pub coordinates: Option<PackageCoordinates>, // Coordinates might be sent with the first chunk or separately
}
