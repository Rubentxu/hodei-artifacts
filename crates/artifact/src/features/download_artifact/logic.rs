use shared::IsoTimestamp;
use crate::{
    application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher},
    error::ArtifactError,
};
use super::query::{GetArtifactQuery, GetArtifactResponse, DownloadMethod};

/// Lógica de negocio para la descarga de artifacts
pub async fn handle_get_artifact(
    query: GetArtifactQuery,
    artifact_repo: &dyn ArtifactRepository,
    artifact_storage: &dyn ArtifactStorage,
    event_publisher: &dyn ArtifactEventPublisher,
) -> Result<GetArtifactResponse, ArtifactError> {
    // 1. Buscar el artifact en la base de datos
    let artifact = artifact_repo
        .get(&query.artifact_id)
        .await?
        .ok_or(ArtifactError::NotFound)?;

    // 2. Publicar evento de descarga solicitada
    let download_event = shared::domain::event::ArtifactDownloadRequested {
        artifact_id: query.artifact_id,
        user_id: query.user_id,
        user_agent: query.user_agent.clone(),
        client_ip: query.client_ip.clone(),
        requested_range: None, // Para futuras implementaciones de partial downloads
    };

    let envelope = shared::domain::event::new_artifact_download_requested_root(download_event);
    
    // Publicar el evento (no blocking - error se logea pero no bloquea descarga)
    if let Err(e) = event_publisher.publish_download_requested(&envelope).await {
        tracing::warn!("Failed to publish download event: {}", e);
        // No retornamos error para no bloquear la descarga
    }

    // 3. Determinar método de descarga
    let download_method = if query.use_presigned_url {
        let expires_secs = query.presigned_expires_secs.unwrap_or(3600); // Default 1 hora
        let url = artifact_storage
            .get_presigned_download_url(&artifact.repository_id, &artifact.id, expires_secs)
            .await?;
        
        let expires_at = (IsoTimestamp::now().0 + chrono::Duration::seconds(expires_secs as i64))
            .to_rfc3339();
        
        DownloadMethod::PresignedUrl { url, expires_at }
    } else {
        let content = artifact_storage
            .get_object_stream(&artifact.repository_id, &artifact.id)
            .await?;
        
        DownloadMethod::Direct { content }
    };

    // 4. Construir respuesta
    Ok(GetArtifactResponse {
        artifact_id: artifact.id,
        file_name: artifact.file_name,
        size_bytes: artifact.size_bytes,
        media_type: None, // No está disponible en el modelo actual, se puede inferir del file_name si es necesario
        checksum: artifact.checksum.0,
        download_method,
    })
}
