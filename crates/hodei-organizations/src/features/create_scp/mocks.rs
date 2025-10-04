use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::error::CreateScpError;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

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

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of ScpPersister for testing
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
