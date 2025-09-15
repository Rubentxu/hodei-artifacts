use super::{
    adapter::{
        KafkaChunkedUploadEventPublisher, MongoChunkedUploadSessionRepository,
        RedisChunkedUploadProgressTracker, S3ChunkStorage,
    },
    api::ChunkedUploadEndpoint,
    ports::*,
    use_case::ChunkedUploadUseCase,
};
use std::sync::Arc;

/// Contenedor de inyección de dependencias para chunked uploads
pub struct ChunkedUploadDIContainer {
    pub endpoint: Arc<ChunkedUploadEndpoint>,
}

impl ChunkedUploadDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        session_repository: Arc<dyn ChunkedUploadSessionRepository>,
        chunk_storage: Arc<dyn ChunkStorage>,
        event_publisher: Arc<dyn ChunkedUploadEventPublisher>,
        progress_tracker: Arc<dyn ChunkedUploadProgressTracker>,
    ) -> Self {
        let use_case = Arc::new(ChunkedUploadUseCase::new(
            session_repository,
            chunk_storage,
            event_publisher,
            progress_tracker,
        ));

        let endpoint = Arc::new(ChunkedUploadEndpoint::new(use_case));

        Self { endpoint }
    }

    /// Método de conveniencia para producción
    pub fn for_production(
        mongo_uri: String,
        s3_bucket: String,
        kafka_brokers: String,
        redis_url: String,
    ) -> Self {
        let session_repository: Arc<dyn ChunkedUploadSessionRepository> =
            Arc::new(MongoChunkedUploadSessionRepository::new(mongo_uri));

        let chunk_storage: Arc<dyn ChunkStorage> = Arc::new(S3ChunkStorage::new(s3_bucket));

        let event_publisher: Arc<dyn ChunkedUploadEventPublisher> =
            Arc::new(KafkaChunkedUploadEventPublisher::new(kafka_brokers));

        let progress_tracker: Arc<dyn ChunkedUploadProgressTracker> =
            Arc::new(RedisChunkedUploadProgressTracker::new(redis_url));

        Self::new(
            session_repository,
            chunk_storage,
            event_publisher,
            progress_tracker,
        )
    }

    /// Método de conveniencia para testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::*;

        let session_repository: Arc<dyn ChunkedUploadSessionRepository> =
            Arc::new(MockChunkedUploadSessionRepository::new());

        let chunk_storage: Arc<dyn ChunkStorage> = Arc::new(MockChunkStorage::new());

        let event_publisher: Arc<dyn ChunkedUploadEventPublisher> =
            Arc::new(MockChunkedUploadEventPublisher::new());

        let progress_tracker: Arc<dyn ChunkedUploadProgressTracker> =
            Arc::new(MockChunkedUploadProgressTracker::new());

        Self::new(
            session_repository,
            chunk_storage,
            event_publisher,
            progress_tracker,
        )
    }
}

/// Configuración para S3
pub struct S3Config {
    pub bucket_name: String,
    pub region: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

impl S3Config {
    pub fn new(bucket_name: String, region: String) -> Self {
        Self {
            bucket_name,
            region,
            access_key: None,
            secret_key: None,
        }
    }

    pub fn with_credentials(mut self, access_key: String, secret_key: String) -> Self {
        self.access_key = Some(access_key);
        self.secret_key = Some(secret_key);
        self
    }
}

/// Configuración para Kafka
pub struct KafkaConfig {
    pub brokers: String,
    pub topic_prefix: String,
}

impl KafkaConfig {
    pub fn new(brokers: String, topic_prefix: String) -> Self {
        Self {
            brokers,
            topic_prefix,
        }
    }
}

/// Configuración para Redis
pub struct RedisConfig {
    pub url: String,
    pub pool_size: u32,
}

impl RedisConfig {
    pub fn new(url: String) -> Self {
        Self { url, pool_size: 10 }
    }

    pub fn with_pool_size(mut self, pool_size: u32) -> Self {
        self.pool_size = pool_size;
        self
    }
}
