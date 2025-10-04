use crate::features::create_scp::error::CreateScpError;
use crate::features::create_scp::ports::{
    CreateScpUnitOfWork, CreateScpUnitOfWorkFactory, ScpPersister,
};
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::shared::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Mock implementation of ScpRepository for testing
#[derive(Debug, Default)]
pub struct MockScpRepository {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepository {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepository for MockScpRepository {
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let mut scps = self.scps.write().unwrap();
        scps.insert(scp.hrn.to_string(), scp.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Deprecated: Use MockCreateScpUnitOfWork instead
#[derive(Debug, Default)]
pub struct MockScpPersister {
    saved_scps: RwLock<Vec<ServiceControlPolicy>>,
}

impl MockScpPersister {
    pub fn new() -> Self {
        Self {
            saved_scps: RwLock::new(Vec::new()),
        }
    }

    pub fn get_saved_scps(&self) -> Vec<ServiceControlPolicy> {
        self.saved_scps.read().unwrap().clone()
    }
}

#[async_trait]
impl ScpPersister for MockScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError> {
        let mut saved_scps = self.saved_scps.write().unwrap();
        saved_scps.push(scp);
        Ok(())
    }
}

/// Mock SCP Repository for testing with failure support
pub struct MockScpRepositoryWithFailure {
    scps: Arc<Mutex<HashMap<String, ServiceControlPolicy>>>,
    should_fail: bool,
}

impl MockScpRepositoryWithFailure {
    pub fn new() -> Self {
        Self {
            scps: Arc::new(Mutex::new(HashMap::new())),
            should_fail: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            scps: Arc::new(Mutex::new(HashMap::new())),
            should_fail,
        }
    }

    pub fn get_saved_scps(&self) -> Vec<ServiceControlPolicy> {
        self.scps.lock().unwrap().values().cloned().collect()
    }
}

#[async_trait]
impl ScpRepository for MockScpRepositoryWithFailure {
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        if self.should_fail {
            return Err(ScpRepositoryError::Storage("Mock failure".to_string()));
        }

        let mut scps = self.scps.lock().unwrap();
        scps.insert(scp.hrn.to_string(), scp.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.lock().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock UnitOfWork for testing transactional behavior
pub struct MockCreateScpUnitOfWork {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<Mutex<Vec<String>>>,
    pub transaction_active: bool,
    scp_repo: Arc<MockScpRepositoryWithFailure>,
}

impl Default for MockCreateScpUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateScpUnitOfWork {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            scp_repo: Arc::new(MockScpRepositoryWithFailure::new()),
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            scp_repo: Arc::new(MockScpRepositoryWithFailure::with_failure(should_fail)),
        }
    }

    pub fn get_saved_scps(&self) -> Vec<ServiceControlPolicy> {
        self.scp_repo.get_saved_scps()
    }
}

#[async_trait]
impl CreateScpUnitOfWork for MockCreateScpUnitOfWork {
    async fn begin(&mut self) -> Result<(), CreateScpError> {
        self.transaction_active = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), CreateScpError> {
        if !self.transaction_active {
            return Err(CreateScpError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CreateScpError> {
        if !self.transaction_active {
            return Err(CreateScpError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    fn scps(&self) -> Arc<dyn ScpRepository> {
        self.scp_repo.clone()
    }
}

/// Mock UnitOfWorkFactory for testing
pub struct MockCreateScpUnitOfWorkFactory {
    pub should_fail_on_save: bool,
}

impl Default for MockCreateScpUnitOfWorkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateScpUnitOfWorkFactory {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
        }
    }
}

#[async_trait]
impl CreateScpUnitOfWorkFactory for MockCreateScpUnitOfWorkFactory {
    type UnitOfWork = MockCreateScpUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateScpError> {
        Ok(MockCreateScpUnitOfWork::with_failure(
            self.should_fail_on_save,
        ))
    }
}
