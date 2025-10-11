use serde::{Deserialize, Serialize};
use shared::enums::HashAlgorithm;
use shared::models::PackageCoordinates;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

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

impl ActionTrait for BatchUploadArtifactCommand {
    fn name() -> &'static str {
        "BatchUploadArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("artifact").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Artifact::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Artifact::Package".to_string()
    }
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
