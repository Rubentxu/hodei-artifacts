use std::sync::Arc;
use std::path::PathBuf;

use super::{
    adapter::{S3ArtifactStorage, MongoDbRepository, RabbitMqEventPublisher, LocalFsChunkedUploadStorage, NoopArtifactValidator},
    ports::{ArtifactRepository, ArtifactStorage, EventPublisher, ChunkedUploadStorage, ArtifactValidator},
    use_case::UploadArtifactUseCase,
    use_case_chunks::UploadArtifactChunkUseCase,
    api::UploadArtifactEndpoint,
};
use crate::features::upload_artifact::upload_progress::{UploadProgressService, UploadProgressDIContainer};
use aws_config::SdkConfig;

/// The Dependency Injection container for the Upload Artifact feature.
pub struct UploadArtifactDIContainer {
    pub endpoint: Arc<UploadArtifactEndpoint>,
    pub use_case: Arc<UploadArtifactUseCase>,
    pub chunk_use_case: Arc<UploadArtifactChunkUseCase>,
}

impl UploadArtifactDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        repository: Arc<dyn ArtifactRepository + Send + Sync>,
        storage: Arc<dyn ArtifactStorage + Send + Sync>,
        publisher: Arc<dyn EventPublisher + Send + Sync>,
        chunked_storage: Arc<dyn ChunkedUploadStorage + Send + Sync>,
        validator: Arc<dyn ArtifactValidator + Send + Sync>,
        progress_service: Arc<UploadProgressService>,
    ) -> Self {
        let use_case = Arc::new(UploadArtifactUseCase::new(repository, storage, publisher.clone(), validator));
        let chunk_use_case = Arc::new(UploadArtifactChunkUseCase::new(chunked_storage, use_case.clone(), publisher, progress_service));
        let endpoint = Arc::new(UploadArtifactEndpoint::new(use_case.clone()));

        Self { endpoint, use_case, chunk_use_case }
    }

    /// Convenience function for wiring up production dependencies.
    pub async fn for_production(
        sdk_config: &SdkConfig,
        mongo_uri: &str,
        db_name: &str,
        amqp_url: &str,
        exchange: &str,
        s3_bucket: &str,
    ) -> Self {
        let repository = Arc::new(MongoDbRepository::new(mongo_uri, db_name).await.unwrap());
        let storage = Arc::new(S3ArtifactStorage::new(sdk_config, s3_bucket.to_string()));
        let publisher = Arc::new(RabbitMqEventPublisher::new(amqp_url, exchange).await.unwrap());

        // Resumable upload support
        let chunk_dir = std::env::var("HODEI_UPLOAD_CHUNKS_DIR").unwrap_or_else(|_| "/tmp/upload_chunks".to_string());
        let chunk_storage = Arc::new(LocalFsChunkedUploadStorage::new(PathBuf::from(chunk_dir)));

        // Default validator (no-op) for MVP
        let validator = Arc::new(NoopArtifactValidator);

        // Progress service para desarrollo (en memoria)
        let progress_container = UploadProgressDIContainer::for_development();
        let progress_service = progress_container.service;

        Self::new(repository, storage, publisher, chunk_storage, validator, progress_service.into())
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> (Self, Arc<super::test_adapter::MockArtifactRepository>, Arc<super::test_adapter::MockArtifactStorage>, Arc<super::test_adapter::MockEventPublisher>, Arc<super::test_adapter::MockArtifactValidator>) {
        use super::test_adapter::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher, MockArtifactValidator};

        let repository = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        // Mock chunked storage would be needed here, but it's not defined yet
        let chunked_storage = Arc::new(LocalFsChunkedUploadStorage::new(PathBuf::from("/tmp/test_chunks")));

        // Progress service para testing
        let progress_container = UploadProgressDIContainer::for_testing();
        let progress_service = progress_container.service;

        (Self::new(repository.clone(), storage.clone(), publisher.clone(), chunked_storage, validator.clone(), progress_service), repository, storage, publisher, validator)
    }
}
