use std::sync::Arc;
use artifact::application::ports::{ArtifactStorage, ArtifactRepository};
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use cedar_policy::{EntityUid, Request, Context, EntityTypeName, EntityId};
use iam::error::IamError;
use std::str::FromStr;

pub async fn handle_maven_download(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    authorization: Arc<dyn Authorization>,
    group_id: String,
    artifact_id: String,
    version: String,
    file_name: String,
) -> Result<Vec<u8>, DistributionError> {
    let artifact = artifact_repository.find_by_maven_coordinates(&group_id, &artifact_id, &version, &file_name).await?
        .ok_or(DistributionError::NotFound)?;

    let repo_id = artifact.repository_id;
    let artifact_id = artifact.id;

    // Authorization check
    let principal = EntityUid::from_type_name_and_id(EntityTypeName::from_str("User")?, EntityId::new("test_user"));
    let action = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Action")?, EntityId::new("read:maven"));
    let resource = EntityUid::from_type_name_and_id(EntityTypeName::from_str("Artifact")?, EntityId::new(&artifact_id.to_string()));
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
        return Err(DistributionError::Iam(IamError::Unauthorized("Not authorized to download artifact".to_string())));
    }

    // Retrieve Artifact
    let bytes = artifact_storage.get_object_stream(&repo_id, &artifact_id).await?;

    Ok(bytes)
}
