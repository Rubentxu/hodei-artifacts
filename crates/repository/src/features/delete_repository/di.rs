// crates/repository/src/features/delete_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use super::ports::{
    RepositoryDeleterPort, RepositoryDeleteAuthorizationPort, ArtifactDeleterPort,
    RepositoryDeleteEventPublisherPort
};
use super::use_case::DeleteRepositoryUseCase;
use super::api::DeleteRepositoryEndpoint;
use super::adapter::{
    MongoDbRepositoryDeleterAdapter, RepositoryDeleteAuthorizationAdapter,
    ArtifactDeleterAdapter, RepositoryDeleteEventPublisherAdapter
};

/// Contenedor de inyección de dependencias para la feature delete_repository
pub struct DeleteRepositoryDIContainer {
    pub endpoint: DeleteRepositoryEndpoint,
}

impl DeleteRepositoryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        repository_deleter_port: Arc<dyn RepositoryDeleterPort>,
        authorization_port: Arc<dyn RepositoryDeleteAuthorizationPort>,
        artifact_deleter_port: Arc<dyn ArtifactDeleterPort>,
        event_publisher_port: Arc<dyn RepositoryDeleteEventPublisherPort>,
    ) -> Self {
        let use_case = Arc::new(DeleteRepositoryUseCase::new(
            repository_deleter_port,
            authorization_port,
            artifact_deleter_port,
            event_publisher_port,
        ));
        
        let endpoint = DeleteRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    /// Constructor para producción con implementaciones MongoDB
    pub fn for_production(db: Database) -> Self {
        let deleter_adapter: Arc<dyn RepositoryDeleterPort> = 
            Arc::new(MongoDbRepositoryDeleterAdapter::new(db.clone()));
        
        let authorization_adapter: Arc<dyn RepositoryDeleteAuthorizationPort> = 
            Arc::new(RepositoryDeleteAuthorizationAdapter::new());
        
        let artifact_deleter_adapter: Arc<dyn ArtifactDeleterPort> = 
            Arc::new(ArtifactDeleterAdapter::new());
        
        let event_publisher_adapter: Arc<dyn RepositoryDeleteEventPublisherPort> = 
            Arc::new(RepositoryDeleteEventPublisherAdapter::new());

        Self::new(
            deleter_adapter,
            authorization_adapter,
            artifact_deleter_adapter,
            event_publisher_adapter,
        )
    }

    /// Constructor para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::test_adapter::{
            MockRepositoryDeleterPort, MockRepositoryDeleteAuthorizationPort,
            MockArtifactDeleterPort, MockRepositoryDeleteEventPublisherPort
        };

        let deleter_port: Arc<dyn RepositoryDeleterPort> = 
            Arc::new(MockRepositoryDeleterPort::new());
        
        let authorization_port: Arc<dyn RepositoryDeleteAuthorizationPort> = 
            Arc::new(MockRepositoryDeleteAuthorizationPort::new());
        
        let artifact_deleter_port: Arc<dyn ArtifactDeleterPort> = 
            Arc::new(MockArtifactDeleterPort::new());
        
        let event_publisher_port: Arc<dyn RepositoryDeleteEventPublisherPort> = 
            Arc::new(MockRepositoryDeleteEventPublisherPort::new());

        Self::new(
            deleter_port,
            authorization_port,
            artifact_deleter_port,
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

    /// Mock para RepositoryDeleterPort
    pub struct MockRepositoryDeleterPort {
        pub should_fail: Mutex<bool>,
        pub should_return_none: Mutex<bool>,
        pub mock_repository: Mutex<Option<Repository>>,
        pub is_empty: Mutex<bool>,
    }

    impl MockRepositoryDeleterPort {
        pub fn new() -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_return_none: Mutex::new(false),
                mock_repository: Mutex::new(None),
                is_empty: Mutex::new(true),
            }
        }

        pub fn with_repository(repository: Repository) -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_return_none: Mutex::new(false),
                mock_repository: Mutex::new(Some(repository)),
                is_empty: Mutex::new(true),
            }
        }

        pub fn with_non_empty_repository(repository: Repository) -> Self {
            Self {
                should_fail: Mutex::new(false),
                should_return_none: Mutex::new(false),
                mock_repository: Mutex::new(Some(repository)),
                is_empty: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl RepositoryDeleterPort for MockRepositoryDeleterPort {
        async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            Ok(())
        }

        async fn get_repository_for_deletion(&self, _repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            if *self.should_return_none.lock().unwrap() {
                return Ok(None);
            }
            
            Ok(self.mock_repository.lock().unwrap().clone())
        }

        async fn is_repository_empty(&self, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            Ok(*self.is_empty.lock().unwrap())
        }
    }

    /// Mock para RepositoryDeleteAuthorizationPort
    pub struct MockRepositoryDeleteAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryDeleteAuthorizationPort {
        pub fn new() -> Self {
            Self {
                should_authorize: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl RepositoryDeleteAuthorizationPort for MockRepositoryDeleteAuthorizationPort {
        async fn can_delete_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    /// Mock para ArtifactDeleterPort
    pub struct MockArtifactDeleterPort {
        pub artifact_count: Mutex<u64>,
        pub should_fail: Mutex<bool>,
    }

    impl MockArtifactDeleterPort {
        pub fn new() -> Self {
            Self {
                artifact_count: Mutex::new(0),
                should_fail: Mutex::new(false),
            }
        }

        pub fn with_artifacts(count: u64) -> Self {
            Self {
                artifact_count: Mutex::new(count),
                should_fail: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl ArtifactDeleterPort for MockArtifactDeleterPort {
        async fn delete_repository_artifacts(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock artifact deletion error".to_string()));
            }
            
            let count = *self.artifact_count.lock().unwrap();
            Ok(count)
        }

        async fn count_repository_artifacts(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock artifact count error".to_string()));
            }
            
            Ok(*self.artifact_count.lock().unwrap())
        }
    }

    /// Mock para RepositoryDeleteEventPublisherPort
    pub struct MockRepositoryDeleteEventPublisherPort {
        pub published_events: Mutex<Vec<(RepositoryId, UserId, u64, u64)>>,
    }

    impl MockRepositoryDeleteEventPublisherPort {
        pub fn new() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RepositoryDeleteEventPublisherPort for MockRepositoryDeleteEventPublisherPort {
        async fn publish_repository_deleted(&self, repository_id: &RepositoryId, deleted_by: &UserId, 
                                           artifact_count: u64, total_size_bytes: u64) -> RepositoryResult<()> {
            self.published_events.lock().unwrap().push((
                repository_id.clone(),
                deleted_by.clone(),
                artifact_count,
                total_size_bytes
            ));
            Ok(())
        }
    }
}