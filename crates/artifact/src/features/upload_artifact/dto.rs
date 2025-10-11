use serde::{Deserialize, Serialize};
use shared::enums::HashAlgorithm;
use shared::models::PackageCoordinates;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

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

impl ActionTrait for UploadArtifactCommand {
    fn name() -> &'static str {
        "UploadArtifact"
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

impl ActionTrait for UploadArtifactChunkCommand {
    fn name() -> &'static str {
        "UploadArtifactChunk"
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
