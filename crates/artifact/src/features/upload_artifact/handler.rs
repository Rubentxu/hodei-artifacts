use crate::features::upload_artifact::command::{UploadArtifactCommand, UploadArtifactResult};
use crate::features::upload_artifact::instrumentation::UploadInstrumentation;
use crate::features::upload_artifact::logic::use_case::{execute_upload_use_case, UploadResult};
use crate::application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher};
use crate::domain::model::ArtifactChecksum;
use crate::error::ArtifactError;

pub async fn handle(
    repo: &dyn ArtifactRepository,
    storage: &dyn ArtifactStorage,
    publisher: &dyn ArtifactEventPublisher,
    cmd: UploadArtifactCommand,
) -> Result<UploadArtifactResult, ArtifactError> {
    // Inicializar instrumentación con correlation ID y span de tracing
    let instrumentation = UploadInstrumentation::new(&cmd.repository_id, &cmd.file_name, cmd.size_bytes);
    let _span = instrumentation.span.clone().entered();

    // 1. Lógica de idempotencia: buscar si el artefacto ya existe.
    let idempotency_start = instrumentation.record_step_start("idempotency_check");
    let checksum = ArtifactChecksum(cmd.checksum.0.clone());
    let existing_artifact = repo.find_by_repo_and_checksum(&cmd.repository_id, &checksum).await?;
    instrumentation.record_step_completion("idempotency_check", idempotency_start);

    // 2. Ejecutar lógica pura del use case
    let use_case_start = instrumentation.record_step_start("use_case_execution");
    let upload_result = execute_upload_use_case(&cmd, existing_artifact)?;
    instrumentation.record_step_completion("use_case_execution", use_case_start);

    // 3. Manejar el resultado según el tipo
    match upload_result {
        UploadResult::AlreadyExists { artifact_id } => {
            instrumentation.record_idempotent_hit();
            Ok(UploadArtifactResult { artifact_id })
        }
        UploadResult::Created { artifact } => {
            // 3. Orquestar efectos secundarios para artefacto nuevo
            // ORDEN MEJORADO: repo.save primero para mejor idempotencia
            
            // 3a. Persistencia en repositorio (PRIMERO para idempotencia)
            let persistence_start = instrumentation.record_step_start("repository_save");
            repo.save(&artifact).await.map_err(|e| {
                instrumentation.record_error(&format!("repository_error: {}", e));
                e
            })?;
            instrumentation.record_step_completion("repository_save", persistence_start);
            
            // 3b. Almacenamiento en storage (SEGUNDO - I/O externa)
            let storage_start = instrumentation.record_step_start("storage_put");
            storage.put_object(&artifact.repository_id, &artifact.id, &cmd.bytes).await.map_err(|e| {
                instrumentation.record_error(&format!("storage_error: {}", e));
                e
            })?;
            instrumentation.record_step_completion("storage_put", storage_start);
            
            // 3c. Publicación de eventos (ÚLTIMO - fire-and-forget)
            let event_start = instrumentation.record_step_start("event_publish");
            publisher.publish_created(&artifact).await.map_err(|e| {
                instrumentation.record_error(&format!("event_publish_error: {}", e));
                e
            })?;
            instrumentation.record_step_completion("event_publish", event_start);

            // 4. Registrar éxito y devolver resultado
            instrumentation.record_created();
            Ok(UploadArtifactResult { artifact_id: artifact.id })
        }
    }
}
