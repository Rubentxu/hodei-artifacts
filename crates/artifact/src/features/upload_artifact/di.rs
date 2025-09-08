use std::sync::Arc;
use std::path::PathBuf;

use super::{
    adapter::{S3ArtifactStorage, MongoDbRepository, RabbitMqEventPublisher, LocalFsChunkedUploadStorage},
    ports::{ArtifactRepository, ArtifactStorage, EventPublisher, ChunkedUploadStorage},
    use_case::UploadArtifactUseCase,
    use_case_chunks::UploadArtifactChunkUseCase,
    api::UploadArtifactEndpoint,
};
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
    ) -> Self {
        let use_case = Arc::new(UploadArtifactUseCase::new(repository, storage, publisher.clone()));
        let chunk_use_case = Arc::new(UploadArtifactChunkUseCase::new(chunked_storage, use_case.clone(), publisher));
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

        Self::new(repository, storage, publisher, chunk_storage)
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> (Self, Arc<super::test_adapter::MockArtifactRepository>, Arc<super::test_adapter::MockArtifactStorage>, Arc<super::test_adapter::MockEventPublisher>) {
        use super::test_adapter::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher};

        let repository = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        // Mock chunked storage would be needed here, but it's not defined yet
        let chunked_storage = Arc::new(LocalFsChunkedUploadStorage::new(PathBuf::from("/tmp/test_chunks")));

        (Self::new(repository.clone(), storage.clone(), publisher.clone(), chunked_storage), repository, storage, publisher)
    }
}
