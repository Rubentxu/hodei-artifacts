use std::sync::Arc;
use crate::features::upload_artifact::ports::{ArtifactRepository, ArtifactStorage, EventPublisher};
use super::{
    use_case::ExtractMetadataUseCase,
    adapter::{RepositoryMetadataUpdater, EventBusMetadataPublisher, StorageArtifactContentReader},
    event_handler::PackageVersionPublishedEventHandler,
    ports::{LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe, MetadataEventPublisher, ArtifactContentReader},
};

/// Dependency injection container for the metadata extraction feature
pub struct ExtractMetadataDIContainer {
    pub event_handler: PackageVersionPublishedEventHandler,
}

impl ExtractMetadataDIContainer {
    /// Create a new DI container with the required dependencies
    pub fn new(
        repository: Arc<dyn ArtifactRepository>,
        storage: Arc<dyn ArtifactStorage>,
        _event_publisher: Arc<dyn EventPublisher>,
    ) -> Self {
        // Create adapters
        let metadata_updater = Arc::new(RepositoryMetadataUpdater::new(repository.clone()));
        let content_reader = Arc::new(StorageArtifactContentReader::new(storage));
        let metadata_publisher = Arc::new(EventBusMetadataPublisher::new());
        
        // Create use case
        let use_case = Arc::new(ExtractMetadataUseCase::new(
            metadata_updater as Arc<dyn LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe>,
            content_reader as Arc<dyn ArtifactContentReader>,
            metadata_publisher as Arc<dyn MetadataEventPublisher>,
        ));
        
        // Create event handler
        let event_handler = PackageVersionPublishedEventHandler::new(use_case.clone());
        
        Self { event_handler }
    }
    
    /// Create a DI container for testing purposes
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use crate::features::upload_artifact::mocks::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher};
        
        let repository = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let event_publisher = Arc::new(MockEventPublisher::new());
        
        Self::new(repository, storage, event_publisher)
    }
}