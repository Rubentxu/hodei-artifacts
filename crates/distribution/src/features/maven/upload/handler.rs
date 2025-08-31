use std::sync::Arc;
use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher, NewArtifactParams};
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use shared::{RepositoryId, UserId, IsoTimestamp};
use shared::domain::event::{ArtifactUploaded, new_artifact_uploaded_root};
use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates};
use cedar_policy::{EntityUid, Request, Context, EntityTypeName, EntityId};
use iam::error::IamError;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use md5::Md5;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub message: String,
    pub artifact_id: String,
    pub sha256: String,
    pub md5: String,
}

pub async fn handle_maven_upload(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    artifact_event_publisher: Arc<dyn ArtifactEventPublisher>,
    authorization: Arc<dyn Authorization>,
    repository_id: RepositoryId,
    group_id: String,
    artifact_id: String,
    version: String,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<UploadResponse, DistributionError> {
    let user_id = UserId::new(); // Placeholder, should be derived from authenticated user

    // Authorization check
    let principal = EntityUid::from_type_name_and_id(EntityTypeName::from_str("User").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("test_user"));
    let action = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Action").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("write:maven"));
    let resource = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Artifact").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new(&format!("{}.{}.{}", group_id, artifact_id, version)));
    let context = Context::empty();

    let request = Request::new(
        principal,
        action,
        resource,
        context,
        None,
    )?;

    let auth_response = authorization.is_authorized(request).await?;

    if auth_response.decision() != cedar_policy::Decision::Allow {
        return Err(DistributionError::Iam(IamError::Unauthorized("Not authorized to upload artifact".to_string())));
    }

    // Calculate checksums
    let sha256_hasher = Sha256::new().chain_update(&bytes);
    let sha256_checksum = format!("{:x}", sha256_hasher.finalize());

    let md5_hasher = Md5::new().chain_update(&bytes);
    let md5_checksum = format!("{:x}", md5_hasher.finalize());

    let artifact_checksum = ArtifactChecksum::new(sha256_checksum.clone());

    // Create NewArtifactParams and Artifact metadata
    let package_coords = PackageCoordinates::build(
        Ecosystem::Maven,
        Some(group_id.clone()),
        artifact_id.clone(),
        version.clone(),
        None,
        std::collections::BTreeMap::new(),
    ).map_err(|e| DistributionError::Internal(format!("Failed to build package coordinates: {}", e)))?;

    let new_artifact_params = NewArtifactParams {
        repository_id: repository_id,
        version: ArtifactVersion::new(version.clone()),
        file_name: file_name.clone(),
        size_bytes: bytes.len() as u64,
        checksum: artifact_checksum.clone(),
        created_by: user_id,
        occurred_at: IsoTimestamp::now(),
    };

    let artifact = Artifact::new(
        new_artifact_params.repository_id,
        new_artifact_params.version,
        new_artifact_params.file_name,
        new_artifact_params.size_bytes,
        new_artifact_params.checksum,
        new_artifact_params.created_by,
    ).with_coordinates(package_coords);

    // Save metadata
    artifact_repository.save(&artifact).await?;

    // Upload binary
    artifact_storage.put_object(&repository_id, &artifact.id, &bytes).await?;

    // Publish event
    let event_payload = ArtifactUploaded {
        artifact_id: artifact.id,
        repository_id: artifact.repository_id,
        uploader: artifact.created_by,
        sha256: Some(artifact.checksum.0),
        size_bytes: Some(artifact.size_bytes),
        media_type: None, // Maven artifacts typically don't have explicit media types
        upload_time_ms: Some(artifact.created_at.0.timestamp_millis() as u32),
    };
    let event = new_artifact_uploaded_root(event_payload);
    artifact_event_publisher.publish_created(&event).await?;

    Ok(UploadResponse {
        message: "Artifact uploaded successfully".to_string(),
        artifact_id: artifact.id.to_string(),
        sha256: sha256_checksum,
        md5: md5_checksum,
    })
}