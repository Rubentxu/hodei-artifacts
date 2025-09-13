
// crates/repository/src/features/delete_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use super::ports::{
    RepositoryDeleterPort, RepositoryDeleteAuthorizationPort, ArtifactDeleterPort,
    RepositoryDeleteEventPublisherPort
};
use super::use_case::DeleteRepositoryUseCase;
use super::api::DeleteRepositoryEndpoint;

// Mock implementations for ports that are not yet fully implemented

pub struct RepositoryDeleteAuthorizationAdapter;

impl RepositoryDeleteAuthorizationAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryDeleteAuthorizationPort for RepositoryDeleteAuthorizationAdapter {
    async fn can_delete_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<bool> {
        Ok(true) // Default to authorized for now
    }
}

pub struct ArtifactDeleterAdapter;

impl ArtifactDeleterAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl ArtifactDeleterPort for ArtifactDeleterAdapter {
    async fn delete_repository_artifacts(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<u64> {
        Ok(0)
    }
    async fn count_repository_artifacts(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<u64> {
        Ok(0)
    }
}

pub struct RepositoryDeleteEventPublisherAdapter;

impl RepositoryDeleteEventPublisherAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryDeleteEventPublisherPort for RepositoryDeleteEventPublisherAdapter {
    async fn publish_repository_deleted(&self, _repository_id: &shared::hrn::RepositoryId, _deleted_by: &shared::hrn::UserId, _artifact_count: u64, _total_size_bytes: u64) -> crate::domain::RepositoryResult<()> {
        Ok(())
    }
}


/// Contenedor de inyección de dependencias para la feature delete_repository
pub struct DeleteRepositoryDIContainer {
    pub endpoint: DeleteRepositoryEndpoint,
}

impl DeleteRepositoryDIContainer {
    pub fn new(
        db: Database,
        authorization_port: Arc<dyn RepositoryDeleteAuthorizationPort>,
        artifact_deleter_port: Arc<dyn ArtifactDeleterPort>,
        event_publisher_port: Arc<dyn RepositoryDeleteEventPublisherPort>,
    ) -> Self {
        let mongo_adapter = Arc::new(MongoDbRepositoryAdapter::new(db));
        
        let use_case = Arc::new(DeleteRepositoryUseCase::new(
            mongo_adapter,
            authorization_port,
            artifact_deleter_port,
            event_publisher_port,
        ));
        
        let endpoint = DeleteRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    pub fn for_production(db: Database) -> Self {
        let authorization_adapter: Arc<dyn RepositoryDeleteAuthorizationPort> = 
            Arc::new(RepositoryDeleteAuthorizationAdapter::new());
        
        let artifact_deleter_adapter: Arc<dyn ArtifactDeleterPort> = 
            Arc::new(ArtifactDeleterAdapter::new());
        
        let event_publisher_adapter: Arc<dyn RepositoryDeleteEventPublisherPort> = 
            Arc::new(RepositoryDeleteEventPublisherAdapter::new());

        Self::new(
            db,
            authorization_adapter,
            artifact_deleter_adapter,
            event_publisher_adapter,
        )
    }

    #[cfg(test)]
    pub async fn for_testing() -> Self {
        let db = crate::infrastructure::tests::setup_test_database().await.unwrap();
        let authorization_port = Arc::new(test_adapter::MockRepositoryDeleteAuthorizationPort::new());
        let artifact_deleter_port = Arc::new(test_adapter::MockArtifactDeleterPort::new());
        let event_publisher_port = Arc::new(test_adapter::MockRepositoryDeleteEventPublisherPort::new());

        Self::new(
            db,
            authorization_port,
            artifact_deleter_port,
            event_publisher_port,
        )
    }
}

#[cfg(test)]
pub mod test_adapter {
    use std::sync::Mutex;
    use async_trait::async_trait;
    use shared::hrn::{RepositoryId, UserId};
    use crate::domain::{RepositoryResult, RepositoryError};
    use crate::domain::repository::Repository;
    use super::super::ports::*;

    pub struct MockRepositoryDeleteAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryDeleteAuthorizationPort {
        pub fn new() -> Self {
            Self { should_authorize: Mutex::new(true) }
        }
    }

    #[async_trait]
    impl RepositoryDeleteAuthorizationPort for MockRepositoryDeleteAuthorizationPort {
        async fn can_delete_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    pub struct MockArtifactDeleterPort;

    impl MockArtifactDeleterPort {
        pub fn new() -> Self { Self }
    }

    #[async_trait]
    impl ArtifactDeleterPort for MockArtifactDeleterPort {
        async fn delete_repository_artifacts(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> { Ok(0) }
        async fn count_repository_artifacts(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> { Ok(0) }
    }

    pub struct MockRepositoryDeleteEventPublisherPort;

    impl MockRepositoryDeleteEventPublisherPort {
        pub fn new() -> Self { Self }
    }

    #[async_trait]
    impl RepositoryDeleteEventPublisherPort for MockRepositoryDeleteEventPublisherPort {
        async fn publish_repository_deleted(&self, _repository_id: &RepositoryId, _deleted_by: &UserId, _artifact_count: u64, _total_size_bytes: u64) -> RepositoryResult<()> {
            Ok(())
        }
    }
}
