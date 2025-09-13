use std::sync::Arc;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use axum::body::to_bytes;
use tower::ServiceExt;

use artifact::features::upload_progress::{
    api::UploadProgressApi,
    use_case::UploadProgressUseCase,
    ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier},
    dto::{UploadStatus, UpdateProgressCommand},
};
use artifact::features::upload_artifact::{
    adapter::LocalFsChunkedUploadStorage,
    ports::ChunkedUploadStorage,
};

// Mocks para las pruebas de la API
#[derive(Default)]
struct MockProgressStorage {
    sessions: std::sync::Mutex<std::collections::HashMap<String, artifact::features::upload_progress::dto::UploadProgress>>,
}

#[async_trait::async_trait]
impl ProgressStorage for MockProgressStorage {
    async fn create_session(&self, progress: artifact::features::upload_progress::dto::UploadProgress) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.sessions.lock().unwrap().insert(progress.upload_id.clone(), progress);
        Ok(())
    }

    async fn get_progress(&self, upload_id: &str) -> Result<artifact::features::upload_progress::dto::UploadProgress, artifact::features::upload_progress::error::ProgressError> {
        self.sessions.lock().unwrap().get(upload_id)
            .cloned()
            .ok_or_else(|| artifact::features::upload_progress::error::ProgressError::SessionNotFound(upload_id.to_string()))
    }

    async fn update_progress(&self, command: artifact::features::upload_progress::dto::UpdateProgressCommand) -> Result<artifact::features::upload_progress::dto::UploadProgress, artifact::features::upload_progress::error::ProgressError> {
        let mut sessions = self.sessions.lock().unwrap();
        let progress = sessions.get_mut(&command.upload_id)
            .ok_or_else(|| artifact::features::upload_progress::error::ProgressError::SessionNotFound(command.upload_id.clone()))?;

        progress.update(command.bytes_transferred, command.total_bytes);
        progress.status = command.status;
        
        Ok(progress.clone())
    }

    async fn delete_session(&self, upload_id: &str) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.sessions.lock().unwrap().remove(upload_id);
        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<artifact::features::upload_progress::dto::UploadProgress>, artifact::features::upload_progress::error::ProgressError> {
        Ok(self.sessions.lock().unwrap().values().cloned().collect())
    }
}

#[derive(Default)]
struct MockEventPublisher {
    published_events: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl ProgressEventPublisher for MockEventPublisher {
    async fn publish_progress_update(&self, _progress: &artifact::features::upload_progress::dto::UploadProgress) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events.lock().unwrap().push("progress_update".to_string());
        Ok(())
    }

    async fn publish_upload_completed(&self, upload_id: &str) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events.lock().unwrap().push(format!("completed_{}", upload_id));
        Ok(())
    }

    async fn publish_upload_failed(&self, upload_id: &str, _error: &str) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events.lock().unwrap().push(format!("failed_{}", upload_id));
        Ok(())
    }
}

#[derive(Default)]
struct MockRealtimeNotifier {
    notifications: std::sync::Mutex<Vec<String>>,
    subscriptions: std::sync::Mutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl RealtimeNotifier for MockRealtimeNotifier {
    async fn notify_progress_update(&self, progress: &artifact::features::upload_progress::dto::UploadProgress) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.notifications.lock().unwrap().push(format!("notify_{}_{}", progress.upload_id, progress.percentage));
        Ok(())
    }

    async fn subscribe(&self, upload_id: &str, client_id: &str) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.subscriptions.lock().unwrap().push((upload_id.to_string(), client_id.to_string()));
        Ok(())
    }

    async fn unsubscribe(&self, client_id: &str) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        let mut subscriptions = self.subscriptions.lock().unwrap();
        subscriptions.retain(|(_, cid)| cid != client_id);
        Ok(())
    }
}

// Mock de identidad de usuario para las pruebas
#[derive(Clone)]
struct MockUserIdentity {
    user_id: String,
}

impl MockUserIdentity {
    fn new(user_id: &str) -> Self {
        Self { user_id: user_id.to_string() }
    }
}

// Implementar el extractor para el mock
impl axum::extract::FromRequestParts<()> for MockUserIdentity {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        _parts: &mut axum::http::request::Parts,
        _state: &()
    ) -> Result<Self, Self::Rejection> {
        Ok(MockUserIdentity::new("test-user"))
    }
}

#[tokio::test]
async fn test_get_received_chunks_api_happy_path() -> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(MockProgressStorage::default());
    let event_publisher = Arc::new(MockEventPublisher::default());
    let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
    
    // Crear almacenamiento de chunks real
    let temp_dir = TempDir::new()?;
    let chunked_storage = Arc::new(LocalFsChunkedUploadStorage::new(temp_dir.path().to_path_buf()));
    
    let use_case = UploadProgressUseCase::new_with_chunked_storage(
        storage.clone(),
        event_publisher,
        realtime_notifier,
        chunked_storage.clone(),
    );
    let api = UploadProgressApi::new(use_case);
    
    // Crear sesión de prueba
    api.use_case.create_session("test-user-chunks-123".to_string(), 3 * 1024 * 1024).await.unwrap();
    
    // Guardar algunos chunks
    let chunk1_data = bytes::Bytes::from("Test chunk 1 data");
    let chunk2_data = bytes::Bytes::from("Test chunk 2 data");
    
    chunked_storage.save_chunk("test-user-chunks-123", 1, chunk1_data).await?;
    chunked_storage.save_chunk("test-user-chunks-123", 2, chunk2_data).await?;
    
    // Crear router con el endpoint
    let app = Router::new()
        .route("/uploads/:upload_id/chunks", get(UploadProgressApi::get_received_chunks))
        .with_state(api);
    
    // Hacer la petición
    let response = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::GET)
                .uri("/uploads/test-user-chunks-123/chunks")
                .body(Body::empty())?,
        )
        .await?;
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = hyper::body::to_bytes(response.into_body()).await?;
    let response_text = std::str::from_utf8(&body)?;
    assert!(response_text.contains("test-user-chunks-123"));
    assert!(response_text.contains(r#""received_chunk_numbers":[1,2]"#));
    
    Ok(())
}

#[tokio::test]
async fn test_get_received_chunks_api_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(MockProgressStorage::default());
    let event_publisher = Arc::new(MockEventPublisher::default());
    let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
    let chunked_storage = Arc::new(LocalFsChunkedUploadStorage::new(TempDir::new()?.path().to_path_buf()));
    
    let use_case = UploadProgressUseCase::new_with_chunked_storage(
        storage.clone(),
        event_publisher,
        realtime_notifier,
        chunked_storage,
    );
    let api = UploadProgressApi::new(use_case);
    
    // Crear router con el endpoint
    let app = Router::new()
        .route("/uploads/:upload_id/chunks", get(UploadProgressApi::get_received_chunks))
        .with_state(api);
    
    // Hacer la petición para un upload que no existe
    let response = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::GET)
                .uri("/uploads/non-existent-upload/chunks")
                .body(Body::empty())?,
        )
        .await?;
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    
    Ok(())
}

#[tokio::test]
async fn test_get_received_chunks_api_unauthorized() -> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(MockProgressStorage::default());
    let event_publisher = Arc::new(MockEventPublisher::default());
    let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
    let chunked_storage = Arc::new(LocalFsChunkedUploadStorage::new(TempDir::new()?.path().to_path_buf()));
    
    let use_case = UploadProgressUseCase::new_with_chunked_storage(
        storage.clone(),
        event_publisher,
        realtime_notifier,
        chunked_storage,
    );
    let api = UploadProgressApi::new(use_case);
    
    // Crear sesión de prueba con un usuario diferente
    api.use_case.create_session("other-user-chunks-456".to_string(), 1000).await.unwrap();
    
    // Crear router con el endpoint
    let app = Router::new()
        .route("/uploads/:upload_id/chunks", get(UploadProgressApi::get_received_chunks))
        .with_state(api);
    
    // Hacer la petición para un upload de otro usuario
    let response = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::GET)
                .uri("/uploads/other-user-chunks-456/chunks")
                .body(Body::empty())?,
        )
        .await?;
    
    // Con la implementación actual de autorización, esto devolverá FORBIDDEN
    // En una implementación real, se verificaría contra una base de datos
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    
    Ok(())
}