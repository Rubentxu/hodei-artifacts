use shared::domain::event::{ArtifactDownloadRequested, DomainEventEnvelope};
use crate::error::ArtifactError;
use super::super::query::GetArtifactQuery;

/// Construir evento de descarga solicitada
pub fn build_download_requested_event(
    query: &GetArtifactQuery,
) -> Result<DomainEventEnvelope<ArtifactDownloadRequested>, ArtifactError> {
    let download_event = ArtifactDownloadRequested {
        artifact_id: query.artifact_id,
        user_id: query.user_id,
        user_agent: query.user_agent.clone(),
        client_ip: query.client_ip.clone(),
        requested_range: None, // Para futuras implementaciones de partial downloads
    };

    let envelope = shared::domain::event::new_artifact_download_requested_root(download_event);
    
    Ok(envelope)
}

/// Construir información de contexto para eventos
#[derive(Debug, Clone)]
pub struct DownloadEventContext {
    pub correlation_id: String,
    pub user_agent: String,
    pub client_ip: String,
    pub download_method: String,
}

impl DownloadEventContext {
    pub fn new(
        correlation_id: String,
        user_agent: String,
        client_ip: String,
        download_method: String,
    ) -> Self {
        Self {
            correlation_id,
            user_agent,
            client_ip,
            download_method,
        }
    }

    pub fn from_query(query: &GetArtifactQuery, correlation_id: String) -> Self {
        let download_method = if query.use_presigned_url {
            "presigned".to_string()
        } else {
            "direct".to_string()
        };

        Self::new(
            correlation_id,
            query.user_agent.clone().unwrap_or("unknown".to_string()),
            query.client_ip.clone().unwrap_or("unknown".to_string()),
            download_method,
        )
    }
}

