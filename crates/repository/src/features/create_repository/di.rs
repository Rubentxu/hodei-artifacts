// crates/repository/src/features/create_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use super::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort,
    StorageBackendExistsPort, EventPublisherPort, RepositoryNameValidatorPort,
    RepositoryConfigValidatorPort
};
use super::use_case::CreateRepositoryUseCase;
use super::api::CreateRepositoryEndpoint;
use super::adapter::{
    MongoDbRepositoryAdapter, EventPublisherAdapter, 
    RepositoryNameValidatorAdapter, RepositoryConfigValidatorAdapter
};

/// Contenedor de inyección de dependencias para la feature create_repository
pub struct CreateRepositoryDIContainer {
    pub endpoint: CreateRepositoryEndpoint,
}

impl CreateRepositoryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        organization_exists_port: Arc<dyn OrganizationExistsPort>,
        repository_exists_port: Arc<dyn RepositoryExistsPort>,
        repository_creator_port: Arc<dyn RepositoryCreatorPort>,
        storage_backend_exists_port: Arc<dyn StorageBackendExistsPort>,
        event_publisher_port: Arc<dyn EventPublisherPort>,
        name_validator_port: Arc<dyn RepositoryNameValidatorPort>,
        config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
    ) -> Self {
        let use_case = Arc::new(CreateRepositoryUseCase::new(
            organization_exists_port,
            repository_exists_port,
            repository_creator_port,
            storage_backend_exists_port,
            event_publisher_port,
            name_validator_port,
            config_validator_port,
        ));
        
        let endpoint = CreateRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    /// Constructor para producción con implementaciones MongoDB
    pub fn for_production(db: Database) -> Self {
        let organization_exists_port: Arc<dyn OrganizationExistsPort> = 
            Arc::new(MongoDbRepositoryAdapter::new(db.clone()));
        
        let repository_exists_port: Arc<dyn RepositoryExistsPort> = 
            Arc::new(MongoDbRepositoryAdapter::new(db.clone()));
        
        let repository_creator_port: Arc<dyn RepositoryCreatorPort> = 
            Arc::new(MongoDbRepositoryAdapter::new(db.clone()));
        
        let storage_backend_exists_port: Arc<dyn StorageBackendExistsPort> = 
            Arc::new(MongoDbRepositoryAdapter::new(db.clone()));
        
        let event_publisher_port: Arc<dyn EventPublisherPort> = 
            Arc::new(EventPublisherAdapter::new());
        
        let name_validator_port: Arc<dyn RepositoryNameValidatorPort> = 
            Arc::new(RepositoryNameValidatorAdapter::new());
        
        let config_validator_port: Arc<dyn RepositoryConfigValidatorPort> = 
            Arc::new(RepositoryConfigValidatorAdapter::new());

        Self::new(
            organization_exists_port,
            repository_exists_port,
            repository_creator_port,
            storage_backend_exists_port,
            event_publisher_port,
            name_validator_port,
            config_validator_port,
        )
    }

    /// Constructor para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::test_adapter::{
            MockOrganizationExistsPort, MockRepositoryExistsPort, MockRepositoryCreatorPort,
            MockStorageBackendExistsPort, MockEventPublisherPort, MockNameValidatorPort,
            MockConfigValidatorPort
        };

        let organization_exists_port: Arc<dyn OrganizationExistsPort> = 
            Arc::new(MockOrganizationExistsPort::new());
        
        let repository_exists_port: Arc<dyn RepositoryExistsPort> = 
            Arc::new(MockRepositoryExistsPort::new());
        
        let repository_creator_port: Arc<dyn RepositoryCreatorPort> = 
            Arc::new(MockRepositoryCreatorPort::new());
        
        let storage_backend_exists_port: Arc<dyn StorageBackendExistsPort> = 
            Arc::new(MockStorageBackendExistsPort::new());
        
        let event_publisher_port: Arc<dyn EventPublisherPort> = 
            Arc::new(MockEventPublisherPort::new());
        
        let name_validator_port: Arc<dyn RepositoryNameValidatorPort> = 
            Arc::new(MockNameValidatorPort::new());
        
        let config_validator_port: Arc<dyn RepositoryConfigValidatorPort> = 
            Arc::new(MockConfigValidatorPort::new());

        Self::new(
            organization_exists_port,
            repository_exists_port,
            repository_creator_port,
            storage_backend_exists_port,
            event_publisher_port,
            name_validator_port,
            config_validator_port,
        )
    }
}

/// Adaptadores de prueba para testing
#[cfg(test)]
pub mod test_adapter {
    use std::sync::Mutex;
    use async_trait::async_trait;
    use shared::hrn::{OrganizationId, RepositoryId};
    use crate::domain::{RepositoryResult, RepositoryError};
    use crate::domain::repository::Repository;
    use super::super::ports::*;

    /// Mock para OrganizationExistsPort
    pub struct MockOrganizationExistsPort {
        pub should_exist: Mutex<bool>,
    }

    impl MockOrganizationExistsPort {
        pub fn new() -> Self {
            Self {
                should_exist: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl OrganizationExistsPort for MockOrganizationExistsPort {
        async fn organization_exists(&self, _organization_id: &OrganizationId) -> RepositoryResult<bool> {
            Ok(*self.should_exist.lock().unwrap())
        }
    }

    /// Mock para RepositoryExistsPort
    pub struct MockRepositoryExistsPort {
        pub should_exist: Mutex<bool>,
    }

    impl MockRepositoryExistsPort {
        pub fn new() -> Self {
            Self {
                should_exist: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl RepositoryExistsPort for MockRepositoryExistsPort {
        async fn repository_exists(&self, _organization_id: &OrganizationId, _name: &str) -> RepositoryResult<bool> {
            Ok(*self.should_exist.lock().unwrap())
        }
    }

    /// Mock para RepositoryCreatorPort
    pub struct MockRepositoryCreatorPort {
        pub should_fail: Mutex<bool>,
        pub created_repositories: Mutex<Vec<String>>,
    }

    impl MockRepositoryCreatorPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                created_repositories: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RepositoryCreatorPort for MockRepositoryCreatorPort {
        async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            self.created_repositories.lock().unwrap().push(repository.hrn.as_str().to_string());
            Ok(())
        }
    }

    /// Mock para StorageBackendExistsPort
    pub struct MockStorageBackendExistsPort {
        pub should_exist: Mutex<bool>,
    }

    impl MockStorageBackendExistsPort {
        pub fn new() -> Self {
            Self {
                should_exist: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl StorageBackendExistsPort for MockStorageBackendExistsPort {
        async fn storage_backend_exists(&self, _storage_backend_hrn: &str) -> RepositoryResult<bool> {
            Ok(*self.should_exist.lock().unwrap())
        }
    }

    /// Mock para EventPublisherPort
    pub struct MockEventPublisherPort {
        pub published_events: Mutex<Vec<String>>,
    }

    impl MockEventPublisherPort {
        pub fn new() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl EventPublisherPort for MockEventPublisherPort {
        async fn publish_repository_created(&self, repository: &Repository) -> RepositoryResult<()> {
            self.published_events.lock().unwrap().push(repository.hrn.as_str().to_string());
            Ok(())
        }
    }

    /// Mock para RepositoryNameValidatorPort
    pub struct MockNameValidatorPort {
        pub should_fail: Mutex<bool>,
        pub failure_message: Mutex<String>,
    }

    impl MockNameValidatorPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                failure_message: Mutex::new("Invalid repository name".to_string()),
            }
        }
    }

    impl RepositoryNameValidatorPort for MockNameValidatorPort {
        fn validate_repository_name(&self, _name: &str) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::InvalidRepositoryName(
                    self.failure_message.lock().unwrap().clone()
                ));
            }
            Ok(())
        }
    }

    /// Mock para RepositoryConfigValidatorPort
    pub struct MockConfigValidatorPort {
        pub should_fail: Mutex<bool>,
        pub failure_message: Mutex<String>,
    }

    impl MockConfigValidatorPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                failure_message: Mutex::new("Invalid repository configuration".to_string()),
            }
        }
    }

    impl RepositoryConfigValidatorPort for MockConfigValidatorPort {
        fn validate_repository_config(
            &self, 
            _repo_type: &crate::domain::repository::RepositoryType, 
            _config: &crate::domain::repository::RepositoryConfig
        ) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::InvalidConfiguration(
                    self.failure_message.lock().unwrap().clone()
                ));
            }
            Ok(())
        }
    }
}