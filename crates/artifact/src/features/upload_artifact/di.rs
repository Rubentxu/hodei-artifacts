use std::sync::Arc;
use super::{
    adapter::{S3ArtifactStorage, MongoDbRepository, RabbitMqEventPublisher},
    ports::{UploadArtifactRepository, ArtifactStorage, EventPublisher},
    use_case::UploadArtifactUseCase,
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

        Self::new(repository, storage, publisher)
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher};

        let repository = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage);
        let publisher = Arc::new(MockEventPublisher::new());

        Self::new(repository, storage, publisher)
    }
}
