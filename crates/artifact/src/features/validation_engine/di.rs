use super::{
    adapter::{
        DefaultValidationRuleExecutor, EventBusValidationPublisher,
        RepositoryValidationRuleManager, StorageArtifactContentReader,
    },
    use_case::ValidationEngineUseCase,
};
use std::sync::Arc;

/// Dependency injection container for validation engine feature
pub struct ValidationEngineDIContainer {
    use_case: ValidationEngineUseCase,
}

impl ValidationEngineDIContainer {
    /// Create a new dependency injection container with real implementations
    pub fn new_with_real_implementations(
        storage: Arc<dyn crate::features::upload_artifact::ports::ArtifactStorage>,
    ) -> Self {
        // Create adapters
        let content_reader = Arc::new(StorageArtifactContentReader::new(storage));
        let rule_repository = Arc::new(RepositoryValidationRuleManager::new());
        let event_publisher = Arc::new(EventBusValidationPublisher::new());
        let rule_executor = Arc::new(DefaultValidationRuleExecutor::new());

        // Create use case
        let use_case = ValidationEngineUseCase::new(
            rule_repository,
            content_reader,
            event_publisher,
            rule_executor,
        );

        Self { use_case }
    }

    /// Create a new dependency injection container with mock implementations for testing
    pub fn new_with_mocks(
        content_reader: Arc<dyn super::ports::ArtifactContentReader>,
        rule_repository: Arc<dyn super::ports::ValidationRuleRepository>,
        event_publisher: Arc<dyn super::ports::ValidationEventPublisher>,
        rule_executor: Arc<dyn super::ports::ValidationRuleExecutor>,
    ) -> Self {
        // Create use case with mocks
        let use_case = ValidationEngineUseCase::new(
            rule_repository,
            content_reader,
            event_publisher,
            rule_executor,
        );

        Self { use_case }
    }

    /// Get the validation engine use case
    pub fn use_case(&self) -> &ValidationEngineUseCase {
        &self.use_case
    }

    /// Consume the container and return the use case
    pub fn into_use_case(self) -> ValidationEngineUseCase {
        self.use_case
    }
}
