// crates/repository/src/features/update_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use super::ports::{
    RepositoryUpdaterPort, RepositoryUpdateAuthorizationPort, RepositoryConfigValidatorPort,
    RepositoryUpdateEventPublisherPort
};
use super::use_case::UpdateRepositoryUseCase;
use super::api::UpdateRepositoryEndpoint;
use super::adapter::{
    MongoDbRepositoryUpdaterAdapter, RepositoryUpdateAuthorizationAdapter,
    RepositoryConfigValidatorAdapter, RepositoryUpdateEventPublisherAdapter
};

/// Contenedor de inyección de dependencias para la feature update_repository
pub struct UpdateRepositoryDIContainer {
    pub endpoint: UpdateRepositoryEndpoint,
}

impl UpdateRepositoryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        repository_updater_port: Arc<dyn RepositoryUpdaterPort>,
        authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
        config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
        event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
    ) -> Self {
        let use_case = Arc::new(UpdateRepositoryUseCase::new(
            repository_updater_port,
            authorization_port,
            config_validator_port,
            event_publisher_port,
        ));
        
        let endpoint = UpdateRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    /// Constructor para producción con implementaciones MongoDB
    pub fn for_production(db: Database) -> Self {
        let updater_adapter: Arc<dyn RepositoryUpdaterPort> = 
            Arc::new(MongoDbRepositoryUpdaterAdapter::new(db.clone()));
        
        let authorization_adapter: Arc<dyn RepositoryUpdateAuthorizationPort> = 
            Arc::new(RepositoryUpdateAuthorizationAdapter::new());
        
        let config_validator_adapter: Arc<dyn RepositoryConfigValidatorPort> = 
            Arc::new(RepositoryConfigValidatorAdapter::new());
        
        let event_publisher_adapter: Arc<dyn RepositoryUpdateEventPublisherPort> = 
            Arc::new(RepositoryUpdateEventPublisherAdapter::new());

        Self::new(
            updater_adapter,
            authorization_adapter,
            config_validator_adapter,
            event_publisher_adapter,
        )
    }

    /// Constructor para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::test_adapter::{
            MockRepositoryUpdaterPort, MockRepositoryUpdateAuthorizationPort,
            MockRepositoryConfigValidatorPort, MockRepositoryUpdateEventPublisherPort
        };

        let updater_port: Arc<dyn RepositoryUpdaterPort> = 
            Arc::new(MockRepositoryUpdaterPort::new());
        
        let authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort> = 
            Arc::new(MockRepositoryUpdateAuthorizationPort::new());
        
        let config_validator_port: Arc<dyn RepositoryConfigValidatorPort> = 
            Arc::new(MockRepositoryConfigValidatorPort::new());
        
        let event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort> = 
            Arc::new(MockRepositoryUpdateEventPublisherPort::new());

        Self::new(
            updater_port,
            authorization_port,
            config_validator_port,
            event_publisher_port,
        )
    }
}

/// Adaptadores de prueba para testing
#[cfg(test)]
pub mod test_adapter {
    use std::sync::Mutex;
    use async_trait::async_trait;
    use shared::hrn::{RepositoryId, UserId};
    use crate::domain::{RepositoryResult, RepositoryError};
    use crate::domain::repository::Repository;
    use super::super::ports::*;

    /// Mock para RepositoryUpdaterPort
    pub struct MockRepositoryUpdaterPort {
        pub should_fail: Mutex<bool>,
        pub should_return_none: Mutex<bool>,
        pub mock_repository: Mutex<Option<Repository>>,
    }

    impl MockRepositoryUpdaterPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_return_none: Mutex::new(false),
                mock_repository: Mutex::new(None),
            }
        }

        pub fn with_repository(repository: Repository) -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_return_none: Mutex::new(false),
                mock_repository: Mutex::new(Some(repository)),
            }
        }
    }

    #[async_trait]
    impl RepositoryUpdaterPort for MockRepositoryUpdaterPort {
        async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            Ok(())
        }

        async fn get_repository_for_update(&self, _repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            if *self.should_return_none.lock().unwrap() {
                return Ok(None);
            }
            
            Ok(self.mock_repository.lock().unwrap().clone())
        }
    }

    /// Mock para RepositoryUpdateAuthorizationPort
    pub struct MockRepositoryUpdateAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryUpdateAuthorizationPort {
        pub fn new() -> Self {
            Self {
                should_authorize: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl RepositoryUpdateAuthorizationPort for MockRepositoryUpdateAuthorizationPort {
        async fn can_update_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    /// Mock para RepositoryConfigValidatorPort
    pub struct MockRepositoryConfigValidatorPort {
        pub should_fail: Mutex<bool>,
        pub should_fail_type_consistency: Mutex<bool>,
    }

    impl MockRepositoryConfigValidatorPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_fail_type_consistency: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl RepositoryConfigValidatorPort for MockRepositoryConfigValidatorPort {
        async fn validate_config_update(&self, _current_config: &crate::domain::repository::RepositoryConfig, 
                                       _new_config: &crate::domain::repository::RepositoryConfig) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::InvalidConfiguration("Mock validation error".to_string()));
            }
            Ok(())
        }

        async fn validate_type_consistency(&self, _current_type: crate::domain::repository::RepositoryType,
                                          _new_type: crate::domain::repository::RepositoryType) -> RepositoryResult<()> {
            if *self.should_fail_type_consistency.lock().unwrap() {
                return Err(RepositoryError::RepositoryTypeMismatch {
                    expected: "Hosted".to_string(),
                    actual: "Proxy".to_string(),
                });
            }
            Ok(())
        }
    }

    /// Mock para RepositoryUpdateEventPublisherPort
    pub struct MockRepositoryUpdateEventPublisherPort {
        pub published_events: Mutex<Vec<(RepositoryId, UserId, Vec<String>)>>,
    }

    impl MockRepositoryUpdateEventPublisherPort {
        pub fn new() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RepositoryUpdateEventPublisherPort for MockRepositoryUpdateEventPublisherPort {
        async fn publish_repository_updated(&self, repository_id: &RepositoryId, updated_by: &UserId, 
                                           changes: Vec<String>) -> RepositoryResult<()> {
            self.published_events.lock().unwrap().push((
                repository_id.clone(),
                updated_by.clone(),
                changes
            ));
            Ok(())
        }
    }
}