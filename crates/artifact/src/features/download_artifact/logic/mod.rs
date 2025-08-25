//! Módulos de lógica pura para download_artifact
//! 
//! Este módulo contiene la lógica de negocio descompuesta en funciones puras
//! siguiendo los principios de Vertical Slice Architecture:
//! 
//! - `validate`: Validaciones puras del query
//! - `use_case`: Lógica de negocio central sin efectos secundarios
//! - `build_event`: Construcción de eventos del dominio

pub mod validate;
pub mod use_case;
pub mod build_event;

use crate::{
    application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher},
    error::ArtifactError,
};
use super::{
    query::{GetArtifactQuery, GetArtifactResponse},
    instrumentation::DownloadInstrumentation,
};

// Re-exportar tipos y funciones principales para facilitar el uso
pub use validate::{validate_download_query, QueryValidationResult};
pub use use_case::{execute_download_use_case, build_download_response, DownloadUseCaseResult};
pub use build_event::{build_download_requested_event, DownloadEventContext};

/// Lógica de negocio para la descarga de artifacts
/// Handler principal que orquesta las validaciones, lógica de negocio y efectos secundarios
pub async fn handle_get_artifact(
    query: GetArtifactQuery,
    artifact_repo: &dyn ArtifactRepository,
    artifact_storage: &dyn ArtifactStorage,
    event_publisher: &dyn ArtifactEventPublisher,
) -> Result<GetArtifactResponse, ArtifactError> {
    // Inicializar instrumentación con correlation ID y span de tracing
    let method = if query.use_presigned_url { "presigned" } else { "direct" };
    let instrumentation = DownloadInstrumentation::new(&query.artifact_id, method);
    let _span = instrumentation.span.clone().entered();

    // 1. Validar query de descarga
    validate_download_query(&query)?;

    // 2. Ejecutar lógica de negocio pura
    let use_case_result = execute_download_use_case(query.clone(), artifact_repo, artifact_storage).await
        .map_err(|e| {
            match e {
                ArtifactError::NotFound => {
                    instrumentation.record_not_found();
                    e
                }
                _ => e,
            }
        })?;

    // 3. Construir y publicar evento de descarga solicitada
    let event_envelope = build_download_requested_event(&query)?;
    
    // Publicar el evento (no blocking - error se logea pero no bloquea descarga)
    if let Err(e) = event_publisher.publish_download_requested(&event_envelope).await {
        tracing::warn!("Failed to publish download event: {}", e);
        instrumentation.record_event_publish_failure("download_requested");
        // No retornamos error para no bloquear la descarga
    }

    // 4. Registrar descarga completada y construir respuesta
    instrumentation.record_completed(method, use_case_result.artifact.size_bytes);
    
    let response = build_download_response(use_case_result);
    Ok(response)
}
