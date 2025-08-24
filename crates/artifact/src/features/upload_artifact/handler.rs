use crate::features::upload_artifact::command::{UploadArtifactCommand, UploadArtifactResult};
use crate::application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher};
use crate::domain::model::{Artifact, ArtifactChecksum};
use crate::error::ArtifactError;

pub async fn handle(
    repo: &dyn ArtifactRepository,
    storage: &dyn ArtifactStorage,
    publisher: &dyn ArtifactEventPublisher,
    cmd: UploadArtifactCommand,
) -> Result<UploadArtifactResult, ArtifactError> {
    // 1. Lógica de idempotencia: buscar si el artefacto ya existe.
    let checksum = ArtifactChecksum(cmd.checksum.0.clone());
    if let Some(existing_artifact) = repo.find_by_repo_and_checksum(&cmd.repository_id, &checksum).await? {
        return Ok(UploadArtifactResult {
            artifact_id: existing_artifact.id,
        });
    }

    // 2. Si no existe, crear y persistir el nuevo artefacto.
    let artifact = Artifact::new(
        cmd.repository_id,
        cmd.version,
        cmd.file_name,
        cmd.size_bytes,
        checksum,
        cmd.user_id,
    );

    // 3. Orquestar efectos secundarios: almacenamiento, persistencia y publicación de eventos.
    storage.put_object(&artifact.repository_id, &artifact.id, &cmd.bytes).await?;
    repo.save(&artifact).await?;
    publisher.publish_created(&artifact).await?;

    // 4. Devolver el ID del artefacto (nuevo o existente).
    Ok(UploadArtifactResult { artifact_id: artifact.id })
}
