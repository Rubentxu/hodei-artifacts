use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use crate::shared::domain::scp::ServiceControlPolicy;

/// Error type for SCP repository operations
#[derive(Debug, thiserror::Error)]
pub enum ScpRepositoryError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Service Control Policy not found: {0}")]
    NotFound(String),
}

/// Repository trait for ServiceControlPolicy entities
#[async_trait]
pub trait ScpRepository: Send + Sync {
    /// Save an SCP
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError>;
    
    /// Find an SCP by HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}
