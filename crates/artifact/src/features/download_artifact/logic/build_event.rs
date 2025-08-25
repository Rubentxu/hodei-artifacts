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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{ArtifactId, UserId};
    use uuid::Uuid;

    fn create_test_query() -> GetArtifactQuery {
        GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            use_presigned_url: false,
            presigned_expires_secs: None,
            user_agent: Some("test-agent/1.0".to_string()),
            client_ip: Some("192.168.1.100".to_string()),
        }
    }

    #[test]
    fn test_build_download_requested_event() {
        let query = create_test_query();
        
        let result = build_download_requested_event(&query);
        
        assert!(result.is_ok());
        let envelope = result.unwrap();
        
        // Verificar que el evento fue construido correctamente
        assert_eq!(envelope.data.artifact_id, query.artifact_id);
        assert_eq!(envelope.data.user_id, query.user_id);
        assert_eq!(envelope.data.user_agent, query.user_agent);
        assert_eq!(envelope.data.client_ip, query.client_ip);
        assert_eq!(envelope.data.requested_range, None);
        
        // Verificar metadatos del envelope
        assert_eq!(envelope.event_type, "ArtifactDownloadRequested.v1");
        assert_ne!(envelope.correlation_id, uuid::Uuid::nil());
        assert_ne!(envelope.event_id, uuid::Uuid::nil());
    }

    #[test]
    fn test_download_event_context_from_query_direct() {
        let query = create_test_query();
        let correlation_id = "test-correlation-123".to_string();
        
        let context = DownloadEventContext::from_query(&query, correlation_id.clone());
        
        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_agent, query.user_agent.unwrap_or("unknown".to_string()));
        assert_eq!(context.client_ip, query.client_ip.unwrap_or("unknown".to_string()));
        assert_eq!(context.download_method, "direct");
    }

    #[test]
    fn test_download_event_context_from_query_presigned() {
        let mut query = create_test_query();
        query.use_presigned_url = true;
        query.presigned_expires_secs = Some(3600);
        
        let correlation_id = "test-correlation-456".to_string();
        
        let context = DownloadEventContext::from_query(&query, correlation_id.clone());
        
        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_agent, query.user_agent.unwrap_or("unknown".to_string()));
        assert_eq!(context.client_ip, query.client_ip.unwrap_or("unknown".to_string()));
        assert_eq!(context.download_method, "presigned");
    }

    #[test]
    fn test_download_event_context_new() {
        let correlation_id = "test-correlation".to_string();
        let user_agent = "custom-agent/2.0".to_string();
        let client_ip = "10.0.0.1".to_string();
        let download_method = "streaming".to_string();
        
        let context = DownloadEventContext::new(
            correlation_id.clone(),
            user_agent.clone(),
            client_ip.clone(),
            download_method.clone(),
        );
        
        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_agent, user_agent);
        assert_eq!(context.client_ip, client_ip);
        assert_eq!(context.download_method, download_method);
    }

    #[test]
    fn test_build_download_requested_event_with_special_characters() {
        let mut query = create_test_query();
        query.user_agent = Some("Mozilla/5.0 (Test Agent)".to_string());
        query.client_ip = Some("2001:db8::1".to_string()); // IPv6 address
        
        let result = build_download_requested_event(&query);
        
        assert!(result.is_ok());
        let envelope = result.unwrap();
        
        assert_eq!(envelope.data.user_agent, query.user_agent);
        assert_eq!(envelope.data.client_ip, query.client_ip);
    }
}
