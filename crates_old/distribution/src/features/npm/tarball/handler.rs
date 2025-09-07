use std::sync::Arc;
use artifact::application::ports::{ArtifactStorage, ArtifactRepository};
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use cedar_policy::{EntityUid, Request, Context, EntityTypeName, EntityId};
use iam::error::IamError;
use std::str::FromStr;

pub async fn handle_npm_tarball_download(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    authorization: Arc<dyn Authorization>,
    package_name: String,
    file_name: String,
) -> Result<Vec<u8>, DistributionError> {
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
        return Err(DistributionError::Iam(IamError::Unauthorized("Not authorized to download npm tarball".to_string())));
    }

    // Retrieve artifact metadata from ArtifactRepository based on package_name and file_name
    let artifacts = artifact_repository.find_by_npm_package_name(&package_name).await?;

    let artifact = artifacts.into_iter().find(|a| a.file_name == file_name)
        .ok_or(DistributionError::NotFound)?;

    // Download artifact binary using ArtifactStorage
    let bytes = artifact_storage.get_object_stream(&artifact.repository_id, &artifact.id).await?;

    Ok(bytes)
}
