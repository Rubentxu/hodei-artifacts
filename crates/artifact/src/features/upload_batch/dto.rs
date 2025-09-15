use serde::{Deserialize, Serialize};
use shared::enums::HashAlgorithm;
use shared::models::PackageCoordinates;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BatchUploadArtifactMetadata {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub checksum: Option<String>,
    pub checksum_algorithm: Option<HashAlgorithm>,
}

#[derive(Debug, Clone)]
pub struct BatchUploadArtifactCommand {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub content_length: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchUploadArtifactResponse {
    pub hrn: String,
    pub url: Option<String>,
    pub status: BatchUploadStatus,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub enum BatchUploadStatus {
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchUploadRequest {
    pub artifacts: Vec<BatchUploadArtifactMetadata>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BatchUploadResponse {
    pub results: Vec<BatchUploadArtifactResponse>,
    pub total_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub skipped_count: usize,
}
