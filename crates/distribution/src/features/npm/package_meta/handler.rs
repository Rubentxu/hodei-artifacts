use std::sync::Arc;
use artifact::application::ports::ArtifactRepository;
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use cedar_policy::{EntityUid, Request, Context, EntityTypeName, EntityId};
use iam::error::IamError;
use std::str::FromStr;
use serde::{Serialize, Deserialize};


#[derive(Debug, Serialize, Deserialize)]
pub struct NpmPackageMetadata {
    pub name: String,
    pub versions: std::collections::BTreeMap<String, NpmVersionMetadata>,
    // TODO: Add more fields as needed for npm metadata
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmVersionMetadata {
    pub version: String,
    pub dist: NpmDistMetadata,
    // TODO: Add more fields as needed for npm version metadata
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmDistMetadata {
    pub shasum: String,
    pub tarball: String,
    // TODO: Add more fields as needed for npm dist metadata
}

pub async fn handle_npm_package_meta(
    artifact_repository: Arc<dyn ArtifactRepository>,
    authorization: Arc<dyn Authorization>,
    package_name: String,
) -> Result<NpmPackageMetadata, DistributionError> {
    // Authorization check
    let principal = EntityUid::from_type_name_and_id(EntityTypeName::from_str("User").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("test_user")); // TODO: Get actual principal from context
    let action = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Action").map_err(|e| DistributionError::CedarParse(e))?, EntityId::new("read:npm"));
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
        return Err(DistributionError::Iam(IamError::Unauthorized("Not authorized to read npm package metadata".to_string())));
    }

    // Retrieve artifact metadata from ArtifactRepository based on package_name
    let artifacts = artifact_repository.find_by_npm_package_name(&package_name).await?;

    if artifacts.is_empty() {
        return Err(DistributionError::NotFound);
    }

    let mut versions = std::collections::BTreeMap::new();
    for artifact in artifacts {
        // Assuming artifact.checksum is SHA1 for npm shasum
        let shasum = artifact.checksum.0;
        let tarball_url = format!("/v2/npm/{}/-/{}", package_name, artifact.file_name);

        let dist = NpmDistMetadata {
            shasum,
            tarball: tarball_url,
        };

        let version_metadata = NpmVersionMetadata {
            version: artifact.version.0,
            dist,
        };
        versions.insert(version_metadata.version.clone(), version_metadata);
    }

    Ok(NpmPackageMetadata {
        name: package_name,
        versions,
    })
}
