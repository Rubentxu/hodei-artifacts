use shared::IsoTimestamp;
use crate::{
    application::ports::{ArtifactRepository, ArtifactStorage},
    domain::model::Artifact,
    error::ArtifactError,
};
use super::super::query::{DownloadMethod, GetArtifactQuery, GetArtifactResponse};

/// Resultado del caso de uso de descarga
#[derive(Debug)]
pub struct DownloadUseCaseResult {
    pub artifact: Artifact,
    pub download_method: DownloadMethod,
}

/// Lógica de negocio pura para la descarga de artifacts
/// Esta función contiene la lógica central sin efectos secundarios de I/O
pub async fn execute_download_use_case(
    query: GetArtifactQuery,
    artifact_repo: &dyn ArtifactRepository,
    artifact_storage: &dyn ArtifactStorage,
) -> Result<DownloadUseCaseResult, ArtifactError> {
    
    // 1. Buscar el artifact en la base de datos
    let artifact = artifact_repo
        .get(&query.artifact_id)
        .await?
        .ok_or(ArtifactError::NotFound)?;

    // 2. Determinar método de descarga basado en el query
    let download_method = if query.use_presigned_url {
        create_presigned_download_method(&query, &artifact, artifact_storage).await?
    } else {
        create_direct_download_method(&artifact, artifact_storage).await?
    };

    Ok(DownloadUseCaseResult {
        artifact,
        download_method,
    })
}

async fn create_presigned_download_method(
    query: &GetArtifactQuery,
    artifact: &Artifact,
    artifact_storage: &dyn ArtifactStorage,
) -> Result<DownloadMethod, ArtifactError> {
    let expires_secs = query.presigned_expires_secs.unwrap_or(3600); // Default 1 hora
    
    let url = artifact_storage
        .get_presigned_download_url(&artifact.repository_id, &artifact.id, expires_secs)
        .await?;
    
    let expires_at = (IsoTimestamp::now().0 + chrono::Duration::seconds(expires_secs as i64))
        .to_rfc3339();
    
    Ok(DownloadMethod::PresignedUrl { url, expires_at })
}

async fn create_direct_download_method(
    artifact: &Artifact,
    artifact_storage: &dyn ArtifactStorage,
) -> Result<DownloadMethod, ArtifactError> {
    let content = artifact_storage
        .get_object_stream(&artifact.repository_id, &artifact.id)
        .await?;
    
    Ok(DownloadMethod::Direct { content })
}

/// Construir respuesta final del query
pub fn build_download_response(result: DownloadUseCaseResult) -> GetArtifactResponse {
    GetArtifactResponse {
        artifact_id: result.artifact.id,
        file_name: result.artifact.file_name,
        size_bytes: result.artifact.size_bytes,
        media_type: None, // No está disponible en el modelo actual, se puede inferir del file_name si es necesario
        checksum: result.artifact.checksum.0,
        download_method: result.download_method,
    }
}

