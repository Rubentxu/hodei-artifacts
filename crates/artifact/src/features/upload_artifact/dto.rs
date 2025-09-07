use serde::{Serialize, Deserialize};
use shared::models::PackageCoordinates;
use shared::enums::HashAlgorithm;

/// Command to upload an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactCommand {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub content_length: u64,
}

/// Minimal metadata received in the multipart form (without content_length).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactMetadata {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    /// Optional checksum hex string provided by client for integrity validation.
    pub checksum: Option<String>,
    /// Optional algorithm, default assumed Sha256 if checksum is provided without algorithm.
    pub checksum_algorithm: Option<HashAlgorithm>,
}

/// Response after a successful upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactResponse {
    /// The unique Hodei Resource Name for the new package version.
    pub hrn: String,
}