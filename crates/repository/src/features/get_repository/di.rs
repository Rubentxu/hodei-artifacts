
// crates/repository/src/features/get_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use super::ports::{
    RepositoryReaderPort, RepositoryAuthorizationPort, RepositoryStatsPort, RepositoryStats
};
use super::use_case::GetRepositoryUseCase;
use super::api::GetRepositoryEndpoint;

// Mock implementations for ports that are not yet fully implemented

pub struct RepositoryAuthorizationAdapter;

impl RepositoryAuthorizationAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryAuthorizationPort for RepositoryAuthorizationAdapter {
    async fn can_read_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<bool> {
        Ok(true) // Default to authorized for now
    }
}

pub struct RepositoryStatsAdapter;

impl RepositoryStatsAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl RepositoryStatsPort for RepositoryStatsAdapter {
    async fn get_artifact_count(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<u64> {
        Ok(0)
    }
    async fn get_total_size(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<u64> {
        Ok(0)
    }
    async fn get_last_upload_date(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<Option<time::OffsetDateTime>> {
        Ok(None)
    }
    async fn get_total_downloads(&self, _repository_id: &shared::hrn::RepositoryId) -> crate::domain::RepositoryResult<u64> {
        Ok(0)
    }
}


/// Contenedor de inyección de dependencias para la feature get_repository
pub struct GetRepositoryDIContainer {
    pub endpoint: GetRepositoryEndpoint,
}

impl GetRepositoryDIContainer {
    pub fn new(
        db: Database,
        authorization_port: Arc<dyn RepositoryAuthorizationPort>,
        stats_port: Arc<dyn RepositoryStatsPort>,
    ) -> Self {
        let mongo_adapter = Arc::new(MongoDbRepositoryAdapter::new(db));
        
        let use_case = Arc::new(GetRepositoryUseCase::new(
            mongo_adapter,
            authorization_port,
            stats_port,
        ));
        
        let endpoint = GetRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    pub fn for_production(db: Database) -> Self {
        let authorization_adapter: Arc<dyn RepositoryAuthorizationPort> = 
            Arc::new(RepositoryAuthorizationAdapter::new());
        
        let stats_adapter: Arc<dyn RepositoryStatsPort> = 
            Arc::new(RepositoryStatsAdapter::new());

        Self::new(
            db,
            authorization_adapter,
            stats_adapter,
        )
    }

    #[cfg(test)]
    pub async fn for_testing() -> Self {
        let db = crate::infrastructure::tests::setup_test_database().await.unwrap();
        let authorization_port = Arc::new(test_adapter::MockRepositoryAuthorizationPort::new());
        let stats_port = Arc::new(test_adapter::MockRepositoryStatsPort::new());

        Self::new(
            db,
            authorization_port,
            stats_port,
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

    pub struct MockRepositoryAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryAuthorizationPort {
        pub fn new() -> Self {
            Self { should_authorize: Mutex::new(true) }
        }
    }

    #[async_trait]
    impl RepositoryAuthorizationPort for MockRepositoryAuthorizationPort {
        async fn can_read_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    pub struct MockRepositoryStatsPort;

    impl MockRepositoryStatsPort {
        pub fn new() -> Self { Self }
    }

    #[async_trait]
    impl RepositoryStatsPort for MockRepositoryStatsPort {
        async fn get_artifact_count(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> { Ok(0) }
        async fn get_total_size(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> { Ok(0) }
        async fn get_last_upload_date(&self, _repository_id: &RepositoryId) -> RepositoryResult<Option<time::OffsetDateTime>> { Ok(None) }
        async fn get_total_downloads(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> { Ok(0) }
    }
}
