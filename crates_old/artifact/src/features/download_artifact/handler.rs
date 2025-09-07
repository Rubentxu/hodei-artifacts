use crate::features::download_artifact::query::{GetArtifactQuery, GetArtifactResponse};
use crate::features::download_artifact::instrumentation::DownloadInstrumentation;
use crate::features::download_artifact::logic::handle_get_artifact;
use crate::application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher};
use crate::error::ArtifactError;

pub async fn handle(
    repo: &dyn ArtifactRepository,
    storage: &dyn ArtifactStorage,
    publisher: &dyn ArtifactEventPublisher,
    query: GetArtifactQuery,
) -> Result<GetArtifactResponse, ArtifactError> {
    // Inicializar instrumentación con correlation ID y span de tracing
    let method = if query.presigned_expires_secs.is_some() { "presigned" } else { "direct" };
    let instrumentation = DownloadInstrumentation::new(&query.artifact_id, method);
    let _span = instrumentation.span.clone().entered();

    // Ejecutar lógica principal a través del módulo logic
    let logic_start = instrumentation.record_step_start("logic_execution");
    let response = handle_get_artifact(query, repo, storage, publisher).await.map_err(|e| {
        instrumentation.record_error(&format!("logic_error: {}", e));
        e
    })?;
    instrumentation.record_step_completion("logic_execution", logic_start);

    // Registrar éxito según el método de descarga
    let size_bytes = response.size_bytes;
    match response.download_method {
        crate::features::download_artifact::query::DownloadMethod::PresignedUrl { .. } => {
            instrumentation.record_completed("presigned", size_bytes);
        }
        crate::features::download_artifact::query::DownloadMethod::Direct { .. } => {
            instrumentation.record_completed("direct", size_bytes);
        }
    }

    Ok(response)
}
