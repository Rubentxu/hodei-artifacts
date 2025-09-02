use std::sync::Arc;
use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher};
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use shared::{RepositoryId, UserId, IsoTimestamp};
use shared::domain::event::{ArtifactUploaded, new_artifact_uploaded_root};
use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates};
use cedar_policy::{EntityUid, Request, Context, EntityTypeName, EntityId};
use iam::error::IamError;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use tar::Archive;
use flate2::read::GzDecoder;
use std::io::Read;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug)]
pub struct NpmTarballUploadRequest {
    pub repository_id: RepositoryId,
    pub user_id: UserId,
    pub package_name: String,
    pub version: String,
    pub tarball_data: Vec<u8>,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
}

pub async fn handle_npm_tarball_upload(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    artifact_event_publisher: Arc<dyn ArtifactEventPublisher>,
    authorization: Arc<dyn Authorization>,
    request: NpmTarballUploadRequest,
) -> Result<Artifact, DistributionError> {
    // Authorization check
    let principal = EntityUid::from_type_name_and_id(
        EntityTypeName::from_str("User").map_err(|e| DistributionError::CedarParse(e))?,
        EntityId::new(&request.user_id.0.to_string()),
    );
    let action = EntityUid::from_type_name_and_id(
        EntityTypeName::from_str("Action").map_err(|e| DistributionError::CedarParse(e))?,
        EntityId::new("write:npm"),
    );
    let resource = EntityUid::from_type_name_and_id(
        EntityTypeName::from_str("Package").map_err(|e| DistributionError::CedarParse(e))?,
        EntityId::new(&request.package_name),
    );
    let context = Context::empty();

    let auth_request = Request::new(principal, action, resource, context, None)?;
    let auth_response = authorization.is_authorized(auth_request).await?;

    if auth_response.decision() != cedar_policy::Decision::Allow {
        return Err(DistributionError::Iam(IamError::Unauthorized(
            "Not authorized to upload npm packages".to_string(),
        )));
    }

    // Extract metadata from tarball
    let (package_json, checksum) = extract_metadata_from_tarball(&request.tarball_data)?;
    
    // Parse package.json to get version and other metadata
    let package_data: Value = serde_json::from_slice(&package_json)
        .map_err(|e| DistributionError::InvalidNpmPackage(format!("Invalid package.json: {}", e)))?;

    let version_str = package_data["version"]
        .as_str()
        .ok_or_else(|| DistributionError::InvalidNpmPackage("Missing version in package.json".to_string()))?
        .to_string();

    if version_str != request.version {
        return Err(DistributionError::InvalidNpmPackage(
            "Version in package.json does not match request version".to_string(),
        ));
    }

    // Create artifact
    let artifact_id = shared::ArtifactId(Uuid::new_v4());
    let file_name = format!("{}-{}.tgz", request.package_name, request.version);

    let checksum_clone = checksum.clone();
    let artifact = Artifact {
        id: artifact_id.clone(),
        repository_id: request.repository_id.clone(),
        version: ArtifactVersion(version_str.clone()),
        file_name: file_name.clone(),
        size_bytes: request.tarball_data.len() as u64,
        checksum: ArtifactChecksum(checksum),
        created_at: IsoTimestamp(chrono::Utc::now()),
        created_by: request.user_id.clone(),
        coordinates: Some(PackageCoordinates::build(
            Ecosystem::Npm,
            None,
            request.package_name.clone(),
            version_str.clone(),
            None,
            std::collections::BTreeMap::new(),
        ).map_err(|e| DistributionError::Internal(format!("Failed to build package coordinates: {}", e)))?),
    };

    // Store artifact in repository
    artifact_repository.save(&artifact).await?;

    // Store tarball in storage
    artifact_storage
        .put_object(&request.repository_id, &artifact_id, &request.tarball_data)
        .await?;

    // Publish artifact created event
    let event_payload = ArtifactUploaded {
        artifact_id: artifact_id.clone(),
        repository_id: request.repository_id.clone(),
        uploader: request.user_id.clone(),
        sha256: Some(checksum_clone),
        size_bytes: Some(request.tarball_data.len() as u64),
        media_type: Some("application/gzip".to_string()),
        upload_time_ms: Some(artifact.created_at.0.timestamp_millis() as u32),
    };

    let event = new_artifact_uploaded_root(event_payload);
    artifact_event_publisher.publish_created(&event).await?;

    Ok(artifact)
}

fn extract_metadata_from_tarball(tarball_data: &[u8]) -> Result<(Vec<u8>, String), DistributionError> {
    // Decompress gzip
    let mut decoder = GzDecoder::new(tarball_data);
    let mut decompressed_data = Vec::new();
    decoder
        .read_to_end(&mut decompressed_data)
        .map_err(|e| DistributionError::InvalidNpmPackage(format!("Failed to decompress tarball: {}", e)))?;

    // Extract tar archive
    let mut archive = Archive::new(&decompressed_data[..]);
    
    for entry in archive.entries().map_err(|e| DistributionError::InvalidNpmPackage(format!("Invalid tar archive: {}", e)))? {
        let mut entry = entry.map_err(|e| DistributionError::InvalidNpmPackage(format!("Invalid tar entry: {}", e)))?;
        
        let path = entry.path().map_err(|e| DistributionError::InvalidNpmPackage(format!("Invalid path in tar: {}", e)))?;
        
        if path.ends_with("package.json") {
            let mut package_json = Vec::new();
            entry.read_to_end(&mut package_json)
                .map_err(|e| DistributionError::InvalidNpmPackage(format!("Failed to read package.json: {}", e)))?;
            
            // Calculate checksum
            let mut hasher = Sha256::new();
            hasher.update(tarball_data);
            let checksum = format!("{:x}", hasher.finalize());
            
            return Ok((package_json, checksum));
        }
    }
    
    Err(DistributionError::InvalidNpmPackage("package.json not found in tarball".to_string()))
}