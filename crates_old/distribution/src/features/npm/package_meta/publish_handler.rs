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
use base64::{engine::general_purpose, Engine as _};
use uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmPublishRequest {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: std::collections::HashMap<String, String>,
    pub versions: std::collections::HashMap<String, NpmVersionData>,
    #[serde(rename = "_attachments")]
    pub attachments: std::collections::HashMap<String, NpmAttachment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmVersionData {
    // This struct will contain the metadata for a specific version
    // It's usually a subset of the main package.json fields
    pub name: String,
    pub version: String,
    pub dist: NpmDistData,
    // Add other fields as needed
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmDistData {
    pub shasum: String,
    pub tarball: String,
    // Add other fields as needed
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmAttachment {
    pub content_type: String,
    pub data: String, // Base64 encoded tarball
    pub length: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmPublishResponse {
    pub success: bool,
    pub id: String,
    pub rev: String,
}

pub async fn handle_npm_publish(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    artifact_event_publisher: Arc<dyn ArtifactEventPublisher>,
    authorization: Arc<dyn Authorization>,
    repository_id: RepositoryId,
    package_name: String,
    bytes: Vec<u8>,
) -> Result<NpmPublishResponse, DistributionError> {
    // Deserialize request body
    let request_body: NpmPublishRequest = serde_json::from_slice(&bytes)
        .map_err(|e| DistributionError::Internal(format!("Failed to parse npm publish request: {}", e)))?;

    let user_id = UserId::new(); // Placeholder, should be derived from authenticated user

    // Authorization check
    let principal = EntityUid::from_type_name_and_id(EntityTypeName::from_str("User").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("test_user")); // TODO: Get actual principal from context
    let action = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Action").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("write:npm"));
    let resource = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Package").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new(&package_name));
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
        return Err(DistributionError::Iam(IamError::Unauthorized("Not authorized to publish npm package".to_string())));
    }

    // Extract tarball from request
    let attachment_key = request_body.attachments.keys().next()
        .ok_or(DistributionError::Internal("No attachment found in npm publish request".to_string()))?;
    let attachment = request_body.attachments.get(attachment_key)
        .ok_or(DistributionError::Internal("Attachment not found".to_string()))?;

    use base64::engine::Engine as _;
use base64::engine::general_purpose;

// ...

    let tarball_bytes = general_purpose::STANDARD.decode(&attachment.data)
        .map_err(|e| DistributionError::Internal(format!("Failed to decode base64 tarball: {}", e)))?;

    // Calculate checksums
    let sha256_hasher = Sha256::new().chain_update(&tarball_bytes);
    let sha256_checksum = format!("{:x}", sha256_hasher.finalize());

    let md5_hasher = Md5::new().chain_update(&tarball_bytes);
    let _md5_checksum = format!("{:x}", md5_hasher.finalize());

    let artifact_checksum = ArtifactChecksum::new(sha256_checksum.clone());

    // Create NewArtifactParams and Artifact metadata
    let version_data = request_body.versions.values().next()
        .ok_or(DistributionError::Internal("No version data found in npm publish request".to_string()))?;

    let package_coords = PackageCoordinates::build(
        Ecosystem::Npm,
        None, // npm packages don't typically have a namespace in the same way Maven does
        request_body.name.clone(),
        version_data.version.clone(),
        None,
        std::collections::BTreeMap::new(),
    ).map_err(|e| DistributionError::Internal(format!("Failed to build package coordinates: {}", e)))?;

    let new_artifact_params = NewArtifactParams {
        repository_id: repository_id,
        version: ArtifactVersion::new(version_data.version.clone()),
        file_name: format!("{}-{}.tgz", request_body.name, version_data.version),
        size_bytes: tarball_bytes.len() as u64,
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
    artifact_storage.put_object(&repository_id, &artifact.id, &tarball_bytes).await?;

    // Publish event
    let event_payload = ArtifactUploaded {
        artifact_id: artifact.id,
        repository_id: artifact.repository_id,
        uploader: artifact.created_by,
        sha256: Some(artifact.checksum.0),
        size_bytes: Some(artifact.size_bytes),
        media_type: Some("application/gzip".to_string()), // npm tarballs are gzipped
        upload_time_ms: Some(artifact.created_at.0.timestamp_millis() as u32),
    };
    let event = new_artifact_uploaded_root(event_payload);
    artifact_event_publisher.publish_created(&event).await?;

    Ok(NpmPublishResponse {
        success: true,
        id: request_body.id,
        rev: "1".to_string(), // TODO: Get actual revision
    })
}

/// Helper function to create an NpmPublishRequest for testing
pub fn create_npm_publish_request(package_name: &str, version: &str, tarball_data: &[u8]) -> NpmPublishRequest {
    let encoded_tarball = general_purpose::STANDARD.encode(tarball_data);
    let mut attachments = std::collections::HashMap::new();
    attachments.insert(
        format!("{}-{}.tgz", package_name, version),
        NpmAttachment {
            content_type: "application/octet-stream".to_string(),
            data: encoded_tarball,
            length: tarball_data.len() as u64,
        },
    );

    let mut versions = std::collections::HashMap::new();
    versions.insert(
        version.to_string(),
        NpmVersionData {
            name: package_name.to_string(),
            version: version.to_string(),
            dist: NpmDistData {
                shasum: "dummy_shasum".to_string(),
                tarball: "dummy_tarball_url".to_string(),
            },
        },
    );

    NpmPublishRequest {
        id: format!("{}", uuid::Uuid::new_v4()),
        name: package_name.to_string(),
        description: Some("A test package".to_string()),
        dist_tags: std::collections::HashMap::new(),
        versions,
        attachments,
    }
}

