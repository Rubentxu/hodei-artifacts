
// crates/repository/src/features/update_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use super::ports::{
    RepositoryUpdaterPort, RepositoryUpdateAuthorizationPort, RepositoryUpdateEventPublisherPort
};
use super::use_case::UpdateRepositoryUseCase;
use super::api::UpdateRepositoryEndpoint;

// Mock implementations for ports that are not yet fully implemented

pub struct RepositoryUpdateAuthorizationAdapter;

impl RepositoryUpdateAuthorizationAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryUpdateAuthorizationPort for RepositoryUpdateAuthorizationAdapter {
    async fn can_update_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<bool> {
        Ok(true) // Default to authorized for now
    }
}

pub struct RepositoryUpdateEventPublisherAdapter;

impl RepositoryUpdateEventPublisherAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryUpdateEventPublisherPort for RepositoryUpdateEventPublisherAdapter {
    async fn publish_repository_updated(&self, _repository_id: &shared::hrn::RepositoryId, _updated_by: &shared::hrn::UserId, _changes: Vec<String>) -> crate::domain::RepositoryResult<()> {
        Ok(())
    }
}


/// Contenedor de inyección de dependencias para la feature update_repository
pub struct UpdateRepositoryDIContainer {
    pub endpoint: UpdateRepositoryEndpoint,
}

impl UpdateRepositoryDIContainer {
    pub fn new(
        db: Database,
        authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
        event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
    ) -> Self {
        let mongo_adapter = Arc::new(MongoDbRepositoryAdapter::new(db));
        
        let use_case = Arc::new(UpdateRepositoryUseCase::new(
            mongo_adapter,
            authorization_port,
            event_publisher_port,
        ));
        
        let endpoint = UpdateRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    pub fn for_production(db: Database) -> Self {
        let authorization_adapter: Arc<dyn RepositoryUpdateAuthorizationPort> = 
            Arc::new(RepositoryUpdateAuthorizationAdapter::new());
        
        let event_publisher_adapter: Arc<dyn RepositoryUpdateEventPublisherPort> = 
            Arc::new(RepositoryUpdateEventPublisherAdapter::new());

        Self::new(
            db,
            authorization_adapter,
            event_publisher_adapter,
        )
    }

    #[cfg(test)]
    pub async fn for_testing() -> Self {
        let db = crate::infrastructure::tests::setup_test_database().await.unwrap();
        let authorization_port = Arc::new(test_adapter::MockRepositoryUpdateAuthorizationPort::new());
        let event_publisher_port = Arc::new(test_adapter::MockRepositoryUpdateEventPublisherPort::new());

        Self::new(
            db,
            authorization_port,
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

    pub struct MockRepositoryUpdateAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryUpdateAuthorizationPort {
        pub fn new() -> Self {
            Self { should_authorize: Mutex::new(true) }
        }
    }

    #[async_trait]
    impl RepositoryUpdateAuthorizationPort for MockRepositoryUpdateAuthorizationPort {
        async fn can_update_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    pub struct MockRepositoryUpdateEventPublisherPort;

    impl MockRepositoryUpdateEventPublisherPort {
        pub fn new() -> Self { Self }
    }

    #[async_trait]
    impl RepositoryUpdateEventPublisherPort for MockRepositoryUpdateEventPublisherPort {
        async fn publish_repository_updated(&self, _repository_id: &RepositoryId, _updated_by: &UserId, _changes: Vec<String>) -> RepositoryResult<()> {
            Ok(())
        }
    }
}
