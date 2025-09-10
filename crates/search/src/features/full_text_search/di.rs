use std::sync::Arc;
use crate::features::full_text_search::{
    use_case::FullTextSearchUseCase,
    ports::{SearchEnginePort, IndexerPort},
    adapters::TantivySearchEngineAdapter,
};

/// Dependency Injection container for Full-Text Search feature
pub struct FullTextSearchDIContainer {
    pub use_case: Arc<FullTextSearchUseCase>,
}

impl FullTextSearchDIContainer {
    /// Create a new DI container with the specified dependencies
    pub fn new(
        search_engine: Arc<dyn SearchEnginePort>,
        indexer: Arc<dyn IndexerPort>,
    ) -> Self {
        let use_case = Arc::new(FullTextSearchUseCase::new(
            search_engine.clone(),
            indexer.clone(),
        ));

        Self { use_case }
    }

    /// Create a DI container for production use with Tantivy adapter
    pub fn for_production() -> Result<Self, Box<dyn std::error::Error>> {
        let tantivy_adapter = Arc::new(TantivySearchEngineAdapter::new()?);
        
        let search_engine = tantivy_adapter.clone() as Arc<dyn SearchEnginePort>;
        let indexer = tantivy_adapter.clone() as Arc<dyn IndexerPort>;

        Ok(Self::new(search_engine, indexer))
    }

    /// Create a DI container for testing with mock dependencies
    #[cfg(test)]
    pub fn for_testing() -> (
        Self,
        Arc<crate::features::full_text_search::test_utils::MockSearchEngineAdapter>,
        Arc<crate::features::full_text_search::test_utils::MockIndexerAdapter>,
    ) {
        use crate::features::full_text_search::test_utils::{MockSearchEngineAdapter, MockIndexerAdapter};
        
        let search_engine = Arc::new(MockSearchEngineAdapter::new());
        let indexer = Arc::new(MockIndexerAdapter::new());

        let container = Self::new(
            search_engine.clone(),
            indexer.clone(),
        );

        (container, search_engine, indexer)
    }
}