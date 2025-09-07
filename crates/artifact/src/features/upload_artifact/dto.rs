use serde::{Serialize, Deserialize};
use shared::models::PackageCoordinates;

/// Command to upload an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactCommand {
    pub coordinates: PackageCoordinates,
    pub file_name: String,
    pub content_length: u64,
}

/// Response after a successful upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactResponse {
    /// The unique Hodei Resource Name for the new package version.
    pub hrn: String,
}