use std::sync::Arc;

use crate::features::basic_search::{
    use_case::BasicSearchUseCase,
    ports::{SearchIndexPort, EventPublisherPort},
    adapter::TantivySearchAdapter,
    event_adapter::LoggingEventPublisherAdapter,
};

/// The Dependency Injection container for the Basic Search feature.
pub struct BasicSearchDIContainer {
    pub use_case: Arc<BasicSearchUseCase>,
}

impl BasicSearchDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        let use_case = Arc::new(BasicSearchUseCase::new(
            search_index,
            event_publisher,
        ));

        Self { use_case }
    }

    /// Convenience function for wiring up production dependencies.
    pub fn for_production() -> Result<Self, Box<dyn std::error::Error>> {
        let search_index = Arc::new(TantivySearchAdapter::new()?);
        let event_publisher = Arc::new(LoggingEventPublisherAdapter::new());

        Ok(Self::new(search_index, event_publisher))
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> (Self, 
        Arc<crate::features::basic_search::test_adapter::MockSearchIndexAdapter>, 
        Arc<crate::features::basic_search::test_adapter::MockEventPublisherAdapter>) {
        use crate::features::basic_search::test_adapter::{MockSearchIndexAdapter, MockEventPublisherAdapter};
        let search_index = Arc::new(MockSearchIndexAdapter::new());
        let event_publisher = Arc::new(MockEventPublisherAdapter::new());

        let container = Self::new(
            search_index.clone(),
            event_publisher.clone(),
        );

        (container, search_index, event_publisher)
    }
}