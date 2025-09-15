use std::sync::Arc;
use tempfile::TempDir;

use artifact::features::upload_artifact::{
    adapter::LocalFsChunkedUploadStorage,
    ports::ChunkedUploadStorage as ArtifactChunkedUploadStorage,
};
use artifact::features::upload_progress::error::ProgressError;
use artifact::features::upload_progress::{
    dto::{UpdateProgressCommand, UploadStatus},
    ports::{ChunkedUploadStorage, ProgressEventPublisher, ProgressStorage, RealtimeNotifier},
    use_case::UploadProgressUseCase,
};

// Adapter to convert ArtifactChunkedUploadStorage to ProgressChunkedUploadStorage
struct ChunkedUploadStorageAdapter {
    inner: Arc<dyn ArtifactChunkedUploadStorage + Send + Sync>,
}

impl ChunkedUploadStorageAdapter {
    fn new(storage: Arc<dyn ArtifactChunkedUploadStorage + Send + Sync>) -> Self {
        Self { inner: storage }
    }
}

#[async_trait::async_trait]
impl ChunkedUploadStorage for ChunkedUploadStorageAdapter {
    async fn save_chunk(
        &self,
        upload_id: &str,
        chunk_number: usize,
        data: bytes::Bytes,
    ) -> Result<(), ProgressError> {
        self.inner
            .save_chunk(upload_id, chunk_number, data)
            .await
            .map_err(|e| ProgressError::StorageError(format!("Failed to save chunk: {:?}", e)))
    }

    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, ProgressError> {
        self.inner
            .get_received_chunks_count(upload_id)
            .await
            .map_err(|e| {
                ProgressError::StorageError(format!("Failed to get chunks count: {:?}", e))
            })
    }

    async fn get_received_chunk_numbers(
        &self,
        upload_id: &str,
    ) -> Result<Vec<usize>, ProgressError> {
        self.inner
            .get_received_chunk_numbers(upload_id)
            .await
            .map_err(|e| {
                ProgressError::StorageError(format!("Failed to get chunk numbers: {:?}", e))
            })
    }

    async fn get_received_chunks_info(
        &self,
        upload_id: &str,
    ) -> Result<Vec<artifact::features::upload_progress::dto::ReceivedChunkInfo>, ProgressError>
    {
        // Since the artifact chunk storage doesn't track individual chunk info,
        // we'll create basic info with dummy size values
        let chunk_numbers = self.get_received_chunk_numbers(upload_id).await?;
        let mut chunks_info = Vec::new();

        for chunk_number in chunk_numbers {
            chunks_info.push(
                artifact::features::upload_progress::dto::ReceivedChunkInfo {
                    chunk_number,
                    size: 1024, // Dummy size value
                },
            );
        }

        Ok(chunks_info)
    }

    async fn assemble_chunks(
        &self,
        upload_id: &str,
        total_chunks: usize,
        file_name: &str,
    ) -> Result<(std::path::PathBuf, String), ProgressError> {
        self.inner
            .assemble_chunks(upload_id, total_chunks, file_name)
            .await
            .map_err(|e| ProgressError::StorageError(format!("Failed to assemble chunks: {:?}", e)))
    }

    async fn cleanup(&self, upload_id: &str) -> Result<(), ProgressError> {
        self.inner
            .cleanup(upload_id)
            .await
            .map_err(|e| ProgressError::StorageError(format!("Failed to cleanup: {:?}", e)))
    }
}

// Mocks para las pruebas de integración
#[derive(Default)]
struct MockProgressStorage {
    sessions: std::sync::Mutex<
        std::collections::HashMap<String, artifact::features::upload_progress::dto::UploadProgress>,
    >,
}

#[async_trait::async_trait]
impl ProgressStorage for MockProgressStorage {
    async fn create_session(
        &self,
        progress: artifact::features::upload_progress::dto::UploadProgress,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(progress.upload_id.clone(), progress);
        Ok(())
    }

    async fn get_progress(
        &self,
        upload_id: &str,
    ) -> Result<
        artifact::features::upload_progress::dto::UploadProgress,
        artifact::features::upload_progress::error::ProgressError,
    > {
        self.sessions
            .lock()
            .unwrap()
            .get(upload_id)
            .cloned()
            .ok_or_else(|| {
                artifact::features::upload_progress::error::ProgressError::SessionNotFound(
                    upload_id.to_string(),
                )
            })
    }

    async fn update_progress(
        &self,
        command: artifact::features::upload_progress::dto::UpdateProgressCommand,
    ) -> Result<
        artifact::features::upload_progress::dto::UploadProgress,
        artifact::features::upload_progress::error::ProgressError,
    > {
        let mut sessions = self.sessions.lock().unwrap();
        let progress = sessions.get_mut(&command.upload_id).ok_or_else(|| {
            artifact::features::upload_progress::error::ProgressError::SessionNotFound(
                command.upload_id.clone(),
            )
        })?;

        progress.update(command.bytes_transferred, command.total_bytes);
        progress.status = command.status;

        Ok(progress.clone())
    }

    async fn delete_session(
        &self,
        upload_id: &str,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.sessions.lock().unwrap().remove(upload_id);
        Ok(())
    }

    async fn list_sessions(
        &self,
    ) -> Result<
        Vec<artifact::features::upload_progress::dto::UploadProgress>,
        artifact::features::upload_progress::error::ProgressError,
    > {
        Ok(self.sessions.lock().unwrap().values().cloned().collect())
    }
}

#[derive(Default)]
struct MockEventPublisher {
    published_events: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl ProgressEventPublisher for MockEventPublisher {
    async fn publish_progress_update(
        &self,
        _progress: &artifact::features::upload_progress::dto::UploadProgress,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events
            .lock()
            .unwrap()
            .push("progress_update".to_string());
        Ok(())
    }

    async fn publish_upload_completed(
        &self,
        upload_id: &str,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events
            .lock()
            .unwrap()
            .push(format!("completed_{}", upload_id));
        Ok(())
    }

    async fn publish_upload_failed(
        &self,
        upload_id: &str,
        _error: &str,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.published_events
            .lock()
            .unwrap()
            .push(format!("failed_{}", upload_id));
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
    async fn notify_progress_update(
        &self,
        progress: &artifact::features::upload_progress::dto::UploadProgress,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.notifications.lock().unwrap().push(format!(
            "notify_{}_{}",
            progress.upload_id, progress.percentage
        ));
        Ok(())
    }

    async fn subscribe(
        &self,
        upload_id: &str,
        client_id: &str,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        self.subscriptions
            .lock()
            .unwrap()
            .push((upload_id.to_string(), client_id.to_string()));
        Ok(())
    }

    async fn unsubscribe(
        &self,
        client_id: &str,
    ) -> Result<(), artifact::features::upload_progress::error::ProgressError> {
        let mut subscriptions = self.subscriptions.lock().unwrap();
        subscriptions.retain(|(_, cid)| cid.as_str() != client_id);
        Ok(())
    }
}

#[tokio::test]
async fn test_upload_progress_service_with_chunks_happy_path()
-> Result<(), Box<dyn std::error::Error>> {
    let storage = Arc::new(MockProgressStorage::default());
    let event_publisher = Arc::new(MockEventPublisher::default());
    let realtime_notifier = Arc::new(MockRealtimeNotifier::default());

    // Crear almacenamiento de chunks real
    let temp_dir = TempDir::new()?;
    let chunked_storage_impl = Arc::new(LocalFsChunkedUploadStorage::new(
        temp_dir.path().to_path_buf(),
    ));
    let chunked_storage = Arc::new(ChunkedUploadStorageAdapter::new(
        chunked_storage_impl.clone(),
    ));

    let use_case = UploadProgressUseCase::new_with_chunked_storage(
        storage.clone(),
        event_publisher.clone(),
        realtime_notifier.clone(),
        chunked_storage.clone(),
    );

    let upload_id = "integration-test-upload-123";
    let total_bytes = 3 * 1024 * 1024; // 3MB

    // Crear sesión de progreso
    use_case
        .create_session(upload_id.to_string(), total_bytes)
        .await?;

    // Guardar algunos chunks
    let chunk1_data = bytes::Bytes::from("Chunk 1 data for integration test");
    let chunk2_data = bytes::Bytes::from("Chunk 2 data for integration test");

    chunked_storage
        .save_chunk(upload_id, 1, chunk1_data)
        .await?;
    chunked_storage
        .save_chunk(upload_id, 2, chunk2_data)
        .await?;

    // Obtener información de chunks recibidos
    let chunks_response = use_case.get_received_chunks(upload_id).await?;

    assert_eq!(chunks_response.upload_id, upload_id);
    assert_eq!(chunks_response.total_chunks, 3); // 3MB / 1MB por chunk
    assert_eq!(chunks_response.received_chunk_numbers, vec![1, 2]);
    assert_eq!(chunks_response.received_chunks.len(), 2);

    // Actualizar progreso
    let progress_command = UpdateProgressCommand {
        upload_id: upload_id.to_string(),
        bytes_transferred: 2 * 1024 * 1024, // 2MB
        total_bytes,
        status: UploadStatus::InProgress,
    };

    let updated_progress = use_case.update_progress(progress_command).await?;
    assert_eq!(updated_progress.bytes_transferred, 2 * 1024 * 1024);
    assert_eq!(updated_progress.percentage, 66); // 2MB / 3MB ≈ 66%

    // Marcar como completado
    let completed_progress = use_case.mark_completed(upload_id).await?;
    assert_eq!(completed_progress.status, UploadStatus::Completed);
    assert_eq!(completed_progress.percentage, 100);

    Ok(())
}

#[tokio::test]
async fn test_upload_progress_service_with_chunks_errors() -> Result<(), Box<dyn std::error::Error>>
{
    let storage = Arc::new(MockProgressStorage::default());
    let event_publisher = Arc::new(MockEventPublisher::default());
    let realtime_notifier = Arc::new(MockRealtimeNotifier::default());

    // Crear almacenamiento de chunks real
    let temp_dir = TempDir::new()?;
    let chunked_storage_impl = Arc::new(LocalFsChunkedUploadStorage::new(
        temp_dir.path().to_path_buf(),
    ));
    let chunked_storage = Arc::new(ChunkedUploadStorageAdapter::new(
        chunked_storage_impl.clone(),
    ));

    let use_case = UploadProgressUseCase::new_with_chunked_storage(
        storage.clone(),
        event_publisher.clone(),
        realtime_notifier.clone(),
        chunked_storage.clone(),
    );

    // Intentar obtener chunks de una sesión que no existe
    let result = use_case.get_received_chunks("non-existent-upload").await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ProgressError::SessionNotFound(_) => {} // Correcto
        _ => panic!("Se esperaba SessionNotFound"),
    }

    // Crear servicio sin chunked storage y probar error
    let use_case_without_chunks = UploadProgressUseCase::new(
        storage.clone(),
        event_publisher.clone(),
        realtime_notifier.clone(),
    );

    let result = use_case_without_chunks
        .get_received_chunks("any-upload")
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ProgressError::StorageError(msg) => {
            assert!(msg.contains("Chunked storage not available"));
        }
        _ => panic!("Se esperaba StorageError"),
    }

    Ok(())
}
