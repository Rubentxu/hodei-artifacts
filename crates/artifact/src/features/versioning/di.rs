use std::sync::Arc;
use super::{
    use_case::VersioningUseCase,
    adapter::{
        RepositoryVersioningAdapter,
        EventBusVersioningPublisher,
        SemverVersionValidator,
    },
    api::VersioningApi,
};

/// Dependency injection container for versioning feature
pub struct VersioningDIContainer {
    use_case: VersioningUseCase,
    api: VersioningApi,
}

impl VersioningDIContainer {
    /// Create a new dependency injection container with real implementations
    pub fn new_with_real_implementations() -> Self {
        // Create adapters
        let repository = Arc::new(RepositoryVersioningAdapter::new());
        let event_publisher = Arc::new(EventBusVersioningPublisher::new());
        let version_validator = Arc::new(SemverVersionValidator::default());
        
        // Create use case
        let use_case = VersioningUseCase::new(
            repository,
            event_publisher,
            version_validator,
        );
        
        // Create API
        let api = VersioningApi::new(use_case.clone());
        
        Self {
            use_case,
            api,
        }
    }
    
    /// Create a new dependency injection container with mock implementations for testing
    pub fn new_with_mocks(
        repository: Arc<dyn super::ports::VersioningRepository>,
        event_publisher: Arc<dyn super::ports::VersioningEventPublisher>,
        version_validator: Arc<dyn super::ports::VersionValidator>,
    ) -> Self {
        // Create use case with mocks
        let use_case = VersioningUseCase::new(
            repository,
            event_publisher,
            version_validator,
        );
        
        // Create API
        let api = VersioningApi::new(use_case.clone());
        
        Self {
            use_case,
            api,
        }
    }
    
    /// Get the versioning use case
    pub fn use_case(&self) -> &VersioningUseCase {
        &self.use_case
    }
    
    /// Get the versioning API
    pub fn api(&self) -> &VersioningApi {
        &self.api
    }
    
    /// Consume the container and return the API
    pub fn into_api(self) -> VersioningApi {
        self.api
    }
    
    /// Consume the container and return the use case
    pub fn into_use_case(self) -> VersioningUseCase {
        self.use_case
    }
}