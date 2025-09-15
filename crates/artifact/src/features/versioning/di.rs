use super::{
    adapter::{EventBusVersioningPublisher, RepositoryVersioningAdapter, SemverVersionValidator},
    use_case::VersioningUseCase,
};
use std::sync::Arc;

/// Dependency injection container for versioning feature
pub struct VersioningDIContainer {
    use_case: VersioningUseCase,
}

impl VersioningDIContainer {
    /// Create a new dependency injection container with real implementations
    pub fn new_with_real_implementations() -> Self {
        // Create adapters
        let repository = Arc::new(RepositoryVersioningAdapter::new());
        let event_publisher = Arc::new(EventBusVersioningPublisher::new());
        let version_validator = Arc::new(SemverVersionValidator::default());

        // Create use case
        let use_case = VersioningUseCase::new(repository, event_publisher, version_validator);

        Self { use_case }
    }

    /// Create a new dependency injection container with mock implementations for testing
    pub fn new_with_mocks(
        repository: Arc<dyn super::ports::VersioningRepository>,
        event_publisher: Arc<dyn super::ports::VersioningEventPublisher>,
        version_validator: Arc<dyn super::ports::VersionValidator>,
    ) -> Self {
        // Create use case with mocks
        let use_case = VersioningUseCase::new(repository, event_publisher, version_validator);

        Self { use_case }
    }

    /// Get the versioning use case
    pub fn use_case(&self) -> &VersioningUseCase {
        &self.use_case
    }

    /// Consume the container and return the use case
    pub fn into_use_case(self) -> VersioningUseCase {
        self.use_case
    }
}
