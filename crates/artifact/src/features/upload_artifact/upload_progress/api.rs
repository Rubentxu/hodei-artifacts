use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use super::{
    UploadProgressService, UploadProgressResponse, UploadProgress,
    ProgressError
};
use crate::features::upload_artifact::api::UserIdentity;

/// API endpoints para el seguimiento de progreso de subidas
#[derive(Clone)]
pub struct UploadProgressApi {
    service: UploadProgressService,
}

impl UploadProgressApi {
    pub fn new(service: UploadProgressService) -> Self {
        Self { service }
    }

    /// Obtener el progreso actual de una subida
    pub async fn get_progress(
        State(api): State<Self>,
        user: UserIdentity,
        Path(upload_id): Path<String>,
    ) -> Box<dyn IntoResponse> {
        info!(upload_id = %upload_id, user_id = %user.user_id, "Getting upload progress");

        let result: Result<UploadProgressResponse, ProgressErrorResponse> = match api.service.get_progress(&upload_id).await {
            Ok(progress) => {
                // Verificar que el usuario tiene acceso a este progreso
                if !is_user_authorized(&progress, &user.user_id) {
                    warn!(upload_id = %upload_id, user_id = %user.user_id, "Unauthorized access to upload progress");
                    return Box::new((StatusCode::FORBIDDEN, Json(ProgressErrorResponse::unauthorized())));
                }

                let response = UploadProgressResponse {
                    progress: progress.clone(),
                    poll_url: Some(format!("/uploads/{}/progress", upload_id)),
                    websocket_url: Some(format!("ws://localhost:3000/uploads/{}/progress/ws", upload_id)),
                };

                info!(upload_id = %upload_id, percentage = progress.percentage, "Progress retrieved successfully");
                Ok(response)
            }
            Err(ProgressError::SessionNotFound(_)) => {
                warn!(upload_id = %upload_id, "Upload session not found");
                Err(ProgressErrorResponse::not_found())
            }
            Err(ProgressError::AccessDenied(_)) => {
                warn!(upload_id = %upload_id, user_id = %user.user_id, "Access denied to upload progress");
                Err(ProgressErrorResponse::unauthorized())
            }
            Err(error) => {
                warn!(upload_id = %upload_id, error = %error, "Error getting upload progress");
                Err(ProgressErrorResponse::internal_error())
            }
        };

        match result {
            Ok(response) => Box::new((StatusCode::OK, Json(response))),
            Err(error_response) => match error_response.error.as_str() {
                "NOT_FOUND" => Box::new((StatusCode::NOT_FOUND, Json(error_response))),
                "UNAUTHORIZED" => Box::new((StatusCode::FORBIDDEN, Json(error_response))),
                _ => Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))),
            }
        }
    }

    /// Listar todas las sesiones de progreso activas (solo admin)
    pub async fn list_sessions(
        State(api): State<Self>,
        user: UserIdentity,
    ) -> Box<dyn IntoResponse> {
        info!(user_id = %user.user_id, "Listing all upload sessions");

        // TODO: Verificar permisos de administrador
        // if !user.is_admin() {
        //     return (StatusCode::FORBIDDEN, Json(ProgressErrorResponse::unauthorized()));
        // }

        match api.service.list_sessions().await {
            Ok(sessions) => {
                info!(session_count = sessions.len(), "Sessions listed successfully");
                Box::new((StatusCode::OK, Json(sessions)))
            }
            Err(error) => {
                warn!(error = %error, "Error listing upload sessions");
                Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(ProgressErrorResponse::internal_error())))
            }
        }
    }

    /// Suscribir cliente to updates de una subida específica (WebSocket)
    pub async fn subscribe_client(
        State(api): State<Self>,
        user: UserIdentity,
        Path(upload_id): Path<String>,
    ) -> Box<dyn IntoResponse> {
        info!(upload_id = %upload_id, user_id = %user.user_id, "Subscribing client to upload progress");

        // Generar ID único de cliente para la suscripción
        let client_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap().to_string();

        match api.service.subscribe_client(&upload_id, &client_id).await {
            Ok(()) => {
                info!(upload_id = %upload_id, client_id = %client_id, "Client subscribed successfully");
                let response = SubscribeResponse {
                    client_id: client_id.clone(),
                    upload_id: upload_id.clone(),
                    websocket_url: format!("ws://localhost:3000/uploads/{}/progress/ws", upload_id),
                };
                Box::new((StatusCode::OK, Json(response)))
            }
            Err(ProgressError::SessionNotFound(_)) => {
                warn!(upload_id = %upload_id, "Upload session not found for subscription");
                Box::new((StatusCode::NOT_FOUND, Json(ProgressErrorResponse::not_found())))
            }
            Err(error) => {
                warn!(upload_id = %upload_id, error = %error, "Error subscribing client");
                Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(ProgressErrorResponse::internal_error())))
            }
        }
    }

    /// Desuscribir cliente de updates
    pub async fn unsubscribe_client(
        State(api): State<Self>,
        user: UserIdentity,
        Path(client_id): Path<String>,
    ) -> Box<dyn IntoResponse> {
        info!(client_id = %client_id, user_id = %user.user_id, "Unsubscribing client from upload progress");

        match api.service.unsubscribe_client(&client_id).await {
            Ok(()) => {
                info!(client_id = %client_id, "Client unsubscribed successfully");
                Box::new((StatusCode::NO_CONTENT, ()))
            }
            Err(error) => {
                warn!(client_id = %client_id, error = %error, "Error unsubscribing client");
                Box::new((StatusCode::INTERNAL_SERVER_ERROR, Json(ProgressErrorResponse::internal_error())))
            }
        }
    }
}

/// Verificar si el usuario tiene acceso al progreso de la subida
fn is_user_authorized(progress: &UploadProgress, user_id: &str) -> bool {
    // TODO: Implementar lógica real de autorización
    // Por ahora, asumimos que el upload_id contiene el user_id o tenemos
    // una relación en base de datos
    progress.upload_id.contains(user_id)
}

/// Response para suscripciones
#[derive(Debug, Serialize)]
struct SubscribeResponse {
    client_id: String,
    upload_id: String,
    websocket_url: String,
}

/// Response de error estandarizado
#[derive(Debug, Serialize)]
struct ProgressErrorResponse {
    error: String,
    code: String,
    message: String,
}

impl ProgressErrorResponse {
    fn not_found() -> Self {
        Self {
            error: "NOT_FOUND".to_string(),
            code: "404".to_string(),
            message: "Upload session not found".to_string(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            error: "UNAUTHORIZED".to_string(),
            code: "403".to_string(),
            message: "Access denied to upload progress".to_string(),
        }
    }

    fn internal_error() -> Self {
        Self {
            error: "INTERNAL_ERROR".to_string(),
            code: "500".to_string(),
            message: "Internal server error".to_string(),
        }
    }
}

/// Request para crear suscripción
#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub upload_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::features::upload_artifact::api::MockUserIdentity;
    use crate::features::upload_artifact::upload_progress::adapter::test::{MockProgressStorage, MockEventPublisher, MockRealtimeNotifier};

    #[tokio::test]
    async fn test_get_progress_authorized() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        let service = UploadProgressService::new(storage.clone(), event_publisher, realtime_notifier);
        let api = UploadProgressApi::new(service.clone());

        // Crear sesión de prueba
        service.create_session("test-user-123".to_string(), 1000).await.unwrap();

        let user = MockUserIdentity::new();
        let response = UploadProgressApi::get_progress(
            State(api.clone()),
            user,
            Path("test-user-123".to_string()),
        ).await;

        let response = (*response).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_progress_unauthorized() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        let service = UploadProgressService::new(storage.clone(), event_publisher, realtime_notifier);
        let api = UploadProgressApi::new(service.clone());

        // Crear sesión de prueba
        service.create_session("other-user-456".to_string(), 1000).await.unwrap();

        let user = MockUserIdentity::new();
        let response = UploadProgressApi::get_progress(
            State(api.clone()),
            user,
            Path("other-user-456".to_string()),
        ).await;

        let response = (*response).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }
}