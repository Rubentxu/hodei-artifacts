use std::sync::Arc;
use tracing::{info, debug, error};

use crate::features::full_text_search::{
    use_case::FullTextSearchUseCase,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort},
    adapter::TantivySearchAdapter,
    repository_adapter::InMemoryArtifactRepositoryAdapter,
    event_adapter::LoggingEventPublisherAdapter,
};

/// The Dependency Injection container for the Full-Text Search feature.
pub struct FullTextSearchDIContainer {
    pub use_case: Arc<FullTextSearchUseCase>,
}

impl FullTextSearchDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        artifact_repository: Arc<dyn ArtifactRepositoryPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        let use_case = Arc::new(FullTextSearchUseCase::new(
            search_index,
            artifact_repository,
            event_publisher,
        ));

        Self { use_case }
    }

    /// Convenience function for wiring up production dependencies.
    pub fn for_production() -> Result<Self, Box<dyn std::error::Error>> {
        let search_index = Arc::new(TantivySearchAdapter::new()?);
        let artifact_repository = Arc::new(InMemoryArtifactRepositoryAdapter::new());
        let event_publisher = Arc::new(LoggingEventPublisherAdapter::new());

        Ok(Self::new(search_index, artifact_repository, event_publisher))
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> (Self, 
        Arc<crate::features::full_text_search::test_utils::MockSearchIndexAdapter>, 
        Arc<crate::features::full_text_search::test_utils::MockArtifactRepositoryAdapter>,
        Arc<crate::features::full_text_search::test_utils::MockEventPublisherAdapter>) {
        use crate::features::full_text_search::test_utils::{MockSearchIndexAdapter, MockArtifactRepositoryAdapter, MockEventPublisherAdapter};
        let search_index = Arc::new(MockSearchIndexAdapter::new());
        let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
        let event_publisher = Arc::new(MockEventPublisherAdapter::new());

        let container = Self::new(
            search_index.clone(),
            artifact_repository.clone(),
            event_publisher.clone(),
        );

        (container, search_index, artifact_repository, event_publisher)
    }
}