use std::path::PathBuf;
use std::sync::Arc;

use super::{
    adapter::{
        LocalFsChunkedUploadStorage, MongoDbRepository, NoopArtifactValidator,
        RabbitMqEventPublisher, S3ArtifactStorage,
    },
    ports::{
        ArtifactRepository, ArtifactStorage, ArtifactValidator, ChunkedUploadStorage,
        EventPublisher, VersionValidator,
    },
    use_case::UploadArtifactUseCase,
};

use crate::features::content_type_detection::{
    ContentTypeDetectionDIContainer, ContentTypeDetectionUseCase,
};
use crate::features::upload_progress::UploadProgressUseCase;
use aws_config::SdkConfig;

/// The Dependency Injection container for the Upload Artifact feature.
pub struct UploadArtifactDIContainer {
    pub use_case: Arc<UploadArtifactUseCase>,
}

impl UploadArtifactDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        repository: Arc<dyn ArtifactRepository + Send + Sync>,
        storage: Arc<dyn ArtifactStorage + Send + Sync>,
        publisher: Arc<dyn EventPublisher + Send + Sync>,
        _chunked_storage: Arc<dyn ChunkedUploadStorage + Send + Sync>, // ya no se usa aquí
        validator: Arc<dyn ArtifactValidator + Send + Sync>,
        _progress_use_case: Arc<UploadProgressUseCase>,
        version_validator: Arc<dyn VersionValidator + Send + Sync>,
        content_type_service: Arc<ContentTypeDetectionUseCase>,
    ) -> Self {
        let use_case = Arc::new(UploadArtifactUseCase::new(
            repository,
            storage,
            publisher.clone(),
            validator,
            version_validator,
            content_type_service,
        ));
        Self { use_case }
    }

    /// Convenience function for wiring up production dependencies.
    #[cfg(not(test))]
    pub async fn from_config(
        config: &SdkConfig,
        mongo_client: mongodb::Client,
        _rabbit_conn: lapin::Connection,
        _upload_dir: PathBuf,
    ) -> Self {
        let repository = Arc::new(MongoDbRepository::new_with_client(mongo_client));
        let storage = Arc::new(S3ArtifactStorage::new(
            config,
            "hodei-artifacts".to_string(),
        ));

        let publisher = Arc::new(
            RabbitMqEventPublisher::new("amqp://localhost", "hodei")
                .await
                .unwrap(),
        );

        // Resumable upload support
        let chunk_dir = std::env::var("HODEI_UPLOAD_CHUNKS_DIR")
            .unwrap_or_else(|_| "/tmp/upload_chunks".to_string());
        let chunk_storage = Arc::new(LocalFsChunkedUploadStorage::new(PathBuf::from(chunk_dir)));

        // Default validator (no-op) for MVP
        let validator = Arc::new(NoopArtifactValidator);

        // Progress service para desarrollo (en memoria)
        let progress_storage = crate::features::upload_progress::di::memory::MemoryProgressStorage::default();
        let event_publisher = crate::features::upload_progress::di::memory::MemoryEventPublisher::default();
        let realtime_notifier = crate::features::upload_progress::di::memory::MemoryRealtimeNotifier::default();
        let progress_use_case = crate::features::upload_progress::UploadProgressUseCase::new(
            Arc::new(progress_storage),
            Arc::new(event_publisher),
            Arc::new(realtime_notifier),
        );

        // Versioning validator
        let version_validator = Arc::new(super::mocks::MockVersionValidator::default());

        // Content-Type detection service
        let content_type_container = ContentTypeDetectionDIContainer::new();
        let content_type_service = content_type_container.use_case;

        Self::new(
            repository,
            storage,
            publisher,
            chunk_storage,
            validator,
            progress_use_case.into(),
            version_validator,
            content_type_service,
        )
    }
}
