// crates/repository/src/features/get_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use super::ports::{
    RepositoryReaderPort, RepositoryAuthorizationPort, RepositoryStatsPort
};
use super::use_case::GetRepositoryUseCase;
use super::api::GetRepositoryEndpoint;
use super::adapter::{
    MongoDbRepositoryReaderAdapter, RepositoryAuthorizationAdapter, RepositoryStatsAdapter
};

/// Contenedor de inyección de dependencias para la feature get_repository
pub struct GetRepositoryDIContainer {
    pub endpoint: GetRepositoryEndpoint,
}

impl GetRepositoryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        repository_reader_port: Arc<dyn RepositoryReaderPort>,
        authorization_port: Arc<dyn RepositoryAuthorizationPort>,
        stats_port: Arc<dyn RepositoryStatsPort>,
    ) -> Self {
        let use_case = Arc::new(GetRepositoryUseCase::new(
            repository_reader_port,
            authorization_port,
            stats_port,
        ));
        
        let endpoint = GetRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    /// Constructor para producción con implementaciones MongoDB
    pub fn for_production(db: Database) -> Self {
        let reader_adapter: Arc<dyn RepositoryReaderPort> = 
            Arc::new(MongoDbRepositoryReaderAdapter::new(db.clone()));
        
        let authorization_adapter: Arc<dyn RepositoryAuthorizationPort> = 
            Arc::new(RepositoryAuthorizationAdapter::new());
        
        let stats_adapter: Arc<dyn RepositoryStatsPort> = 
            Arc::new(RepositoryStatsAdapter::new(reader_adapter.clone()));

        Self::new(
            reader_adapter,
            authorization_adapter,
            stats_adapter,
        )
    }

    /// Constructor para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::test_adapter::{
            MockRepositoryReaderPort, MockRepositoryAuthorizationPort, MockRepositoryStatsPort
        };

        let reader_port: Arc<dyn RepositoryReaderPort> = 
            Arc::new(MockRepositoryReaderPort::new());
        
        let authorization_port: Arc<dyn RepositoryAuthorizationPort> = 
            Arc::new(MockRepositoryAuthorizationPort::new());
        
        let stats_port: Arc<dyn RepositoryStatsPort> = 
            Arc::new(MockRepositoryStatsPort::new());

        Self::new(
            reader_port,
            authorization_port,
            stats_port,
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

    /// Mock para RepositoryReaderPort
    pub struct MockRepositoryReaderPort {
        pub should_return_none: Mutex<bool>,
        pub should_fail: Mutex<bool>,
        pub mock_repository: Mutex<Option<Repository>>,
    }

    impl MockRepositoryReaderPort {
        pub fn new() -> Self {
            Self {
                should_return_none: Mutex::new(false),
                should_fail: Mutex::new(false),
                mock_repository: Mutex::new(None),
            }
        }

        pub fn with_repository(repository: Repository) -> Self {
            Self {
                should_return_none: Mutex::new(false),
                should_fail: Mutex::new(false),
                mock_repository: Mutex::new(Some(repository)),
            }
        }
    }

    #[async_trait]
    impl RepositoryReaderPort for MockRepositoryReaderPort {
        async fn get_repository(&self, _repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            if *self.should_return_none.lock().unwrap() {
                return Ok(None);
            }
            
            Ok(self.mock_repository.lock().unwrap().clone())
        }

        async fn get_repository_stats(&self, _repository_id: &RepositoryId) -> RepositoryResult<RepositoryStats> {
            if *self.should_fail.lock().unwrap() {
                return Err(RepositoryError::DatabaseError("Mock database error".to_string()));
            }
            
            Ok(RepositoryStats {
                artifact_count: 10,
                total_size_bytes: 1024 * 1024, // 1MB
                last_artifact_uploaded_at: Some(time::OffsetDateTime::now_utc()),
                total_downloads: 100,
            })
        }
    }

    /// Mock para RepositoryAuthorizationPort
    pub struct MockRepositoryAuthorizationPort {
        pub should_authorize: Mutex<bool>,
    }

    impl MockRepositoryAuthorizationPort {
        pub fn new() -> Self {
            Self {
                should_authorize: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl RepositoryAuthorizationPort for MockRepositoryAuthorizationPort {
        async fn can_read_repository(&self, _user_id: &UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
            Ok(*self.should_authorize.lock().unwrap())
        }
    }

    /// Mock para RepositoryStatsPort
    pub struct MockRepositoryStatsPort {
        pub artifact_count: Mutex<u64>,
        pub total_size: Mutex<u64>,
        pub last_upload_date: Mutex<Option<time::OffsetDateTime>>,
        pub total_downloads: Mutex<u64>,
    }

    impl MockRepositoryStatsPort {
        pub fn new() -> Self {
            Self {
                artifact_count: Mutex::new(0),
                total_size: Mutex::new(0),
                last_upload_date: Mutex::new(None),
                total_downloads: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl RepositoryStatsPort for MockRepositoryStatsPort {
        async fn get_artifact_count(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> {
            Ok(*self.artifact_count.lock().unwrap())
        }

        async fn get_total_size(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> {
            Ok(*self.total_size.lock().unwrap())
        }

        async fn get_last_upload_date(&self, _repository_id: &RepositoryId) -> RepositoryResult<Option<time::OffsetDateTime>> {
            Ok(*self.last_upload_date.lock().unwrap())
        }

        async fn get_total_downloads(&self, _repository_id: &RepositoryId) -> RepositoryResult<u64> {
            Ok(*self.total_downloads.lock().unwrap())
        }
    }
}