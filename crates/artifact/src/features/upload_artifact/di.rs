use std::sync::Arc;
use super::{
    adapter::{S3ArtifactStorage, MongoDbRepository, RabbitMqEventPublisher, LocalChunkedUploadStorage},
    ports::{UploadArtifactRepository, ArtifactStorage, EventPublisher},
    use_case::{UploadArtifactUseCase, UploadArtifactChunkUseCase},
    api::UploadArtifactEndpoint,
};
use aws_config::SdkConfig;

/// The Dependency Injection container for the Upload Artifact feature.
pub struct UploadArtifactDIContainer {
    pub endpoint: Arc<UploadArtifactEndpoint>,
}

impl UploadArtifactDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        repository: Arc<dyn UploadArtifactRepository>,
        storage: Arc<dyn ArtifactStorage>,
        publisher: Arc<dyn EventPublisher>,
    ) -> Self {
        let use_case = Arc::new(UploadArtifactUseCase::new(repository, storage, publisher));
        let endpoint = Arc::new(UploadArtifactEndpoint::new(use_case));
        
        Self { endpoint }
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

        // Primary use case
        let use_case = Arc::new(UploadArtifactUseCase::new(repository.clone(), storage.clone(), publisher.clone()));

        // Resumable upload support
        let chunk_dir = std::env::var("HODEI_UPLOAD_CHUNKS_DIR").unwrap_or_else(|_| "/tmp/upload_chunks".to_string());
        let chunk_storage = Arc::new(LocalChunkedUploadStorage::new(chunk_dir));
        let chunk_use_case = Arc::new(UploadArtifactChunkUseCase::new(chunk_storage.clone(), publisher.clone()));

        // Endpoint with resumable
        let endpoint = Arc::new(UploadArtifactEndpoint::with_resumable(use_case.clone(), chunk_use_case, chunk_storage));

        Self { endpoint }
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::test_adapter::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher};

        let repository = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());

        Self::new(repository, storage, publisher)
    }
}
