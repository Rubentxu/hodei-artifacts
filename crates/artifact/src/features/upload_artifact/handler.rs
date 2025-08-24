use crate::features::upload_artifact::command::{UploadArtifactCommand, UploadArtifactResult};
use crate::application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher, NewArtifactParams};
use crate::domain::model::{Artifact, ArtifactVersion, ArtifactChecksum};
use crate::error::ArtifactError;
use shared::{IsoTimestamp, ArtifactId};

pub async fn handle(
    repo: &dyn ArtifactRepository,
    storage: &dyn ArtifactStorage,
    publisher: &dyn ArtifactEventPublisher,
    cmd: UploadArtifactCommand,
) -> Result<UploadArtifactResult, ArtifactError> {
    let params = NewArtifactParams {
        repository_id: cmd.repository_id,
        version: ArtifactVersion(cmd.version.0),
        file_name: cmd.file_name.clone(),
        size_bytes: cmd.size_bytes,
        checksum: ArtifactChecksum(cmd.checksum.0),
        created_by: cmd.user_id,
        occurred_at: IsoTimestamp::now(),
    };

    let artifact = Artifact::new(
        params.repository_id,
        params.version,
        params.file_name,
        params.size_bytes,
        params.checksum,
        params.created_by,
    );

    storage.put_object(&artifact.repository_id, &artifact.id, &cmd.bytes).await?;
    repo.save(&artifact).await?;
    publisher.publish_created(&artifact).await?;

    Ok(UploadArtifactResult { artifact_id: ArtifactId(artifact.id.0) })
}

