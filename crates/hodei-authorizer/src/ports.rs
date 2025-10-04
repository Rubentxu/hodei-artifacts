
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;
use std::collections::HashMap;
use policies::domain::Hrn;

/// Port for retrieving IAM policies for a principal
#[async_trait::async_trait]
pub trait IamPolicyProvider: Send + Sync {
    /// Get identity policies for a principal
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError>;
}

/// Port for retrieving effective SCPs for an entity
#[async_trait::async_trait]
pub trait OrganizationBoundaryProvider: Send + Sync {
    /// Get effective SCPs for an entity
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, AuthorizationError>;
}

/// Error type for authorization operations
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("IAM policy provider error: {0}")]
    IamPolicyProvider(String),
    #[error("Organization boundary provider error: {0}")]
    OrganizationBoundaryProvider(String),
}
