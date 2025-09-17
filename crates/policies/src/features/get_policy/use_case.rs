use super::dto::{GetPolicyDetailsResponse, GetPolicyQuery, GetPolicyResponse, PolicyNotFoundResponse};
use super::error::GetPolicyError;
use super::ports::{PolicyAccessValidator, PolicyGetter, PolicyRetrievalAuditor, PolicyRetrievalStorage};
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use chrono::Utc;
use shared::hrn::UserId;
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{info, warn};

/// Use case for getting policies
pub struct GetPolicyUseCase<PAV, PRS, PRA> {
    access_validator: Arc<PAV>,
    storage: Arc<PRS>,
    auditor: Arc<PRA>,
    _phantom: PhantomData<PRA>,
}

impl<PAV, PRS, PRA> GetPolicyUseCase<PAV, PRS, PRA> {
    pub fn new(
        access_validator: Arc<PAV>,
        storage: Arc<PRS>,
        auditor: Arc<PRA>,
    ) -> Self {
        Self {
            access_validator,
            storage,
            auditor,
            _phantom: PhantomData,
        }
    }

    /// Execute the get policy use case
    pub async fn execute(&self, query: GetPolicyQuery, user_id: &UserId) -> Result<GetPolicyResponse, GetPolicyError> {
        info!("Getting policy: {}", query.policy_id);

        // Retrieve the policy
        let policy = if query.include_versions {
            self.storage.find_by_id_with_versions(&query.policy_id).await?
        } else {
            self.storage.find_by_id(&query.policy_id).await?
        };

        let policy = match policy {
            Some(p) => p,
            None => {
                warn!("Policy not found: {}", query.policy_id);
                return Err(GetPolicyError::policy_not_found(query.policy_id));
            }
        };

        // Validate access permissions
        self.access_validator.validate_access(&policy, user_id).await?;

        // Log the access
        self.auditor.log_policy_access(&query.policy_id, user_id).await?;

        info!("Policy retrieved successfully: {}", query.policy_id);

        Ok(GetPolicyResponse {
            policy_id: policy.id,
            name: policy.name,
            description: policy.description,
            status: policy.status,
            version: policy.version,
            content: policy.current_version.content,
            created_at: policy.created_at.to_rfc3339(),
            updated_at: policy.updated_at.to_rfc3339(),
            created_by: policy.current_version.created_by.to_string(),
        })
    }

    /// Get policy with detailed information
    pub async fn get_policy_details(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<GetPolicyDetailsResponse, GetPolicyError> {
        info!("Getting policy details: {}", policy_id);

        // Retrieve policy with versions
        let policy = self.storage.find_by_id_with_versions(policy_id).await?
            .ok_or_else(|| GetPolicyError::policy_not_found(policy_id.clone()))?;

        // Validate access
        self.access_validator.validate_access(&policy, user_id).await?;

        // Log the access
        self.auditor.log_policy_access(policy_id, user_id).await?;

        // TODO: Calculate versions count and last modified by from versions
        let versions_count = 1; // Simplified
        let last_modified_by = Some(policy.current_version.created_by.to_string());

        Ok(GetPolicyDetailsResponse {
            policy_id: policy.id,
            name: policy.name,
            description: policy.description,
            status: policy.status,
            version: policy.version,
            content: policy.current_version.content,
            created_at: policy.created_at.to_rfc3339(),
            updated_at: policy.updated_at.to_rfc3339(),
            created_by: policy.current_version.created_by.to_string(),
            versions_count,
            last_modified_by,
        })
    }
}

#[async_trait]
impl<PAV, PRS, PRA> PolicyGetter for GetPolicyUseCase<PAV, PRS, PRA>
where
    PAV: PolicyAccessValidator + Send + Sync,
    PRS: PolicyRetrievalStorage + Send + Sync,
    PRA: PolicyRetrievalAuditor + Send + Sync,
{
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError> {
        // Create a query for basic retrieval
        let query = GetPolicyQuery {
            policy_id: policy_id.clone(),
            include_versions: false,
        };

        // For the interface, we need to return Option<Policy>
        // This is a simplified implementation
        match self.storage.find_by_id(policy_id).await {
            Ok(policy) => Ok(policy),
            Err(e) => Err(e),
        }
    }

    async fn get_policy_details(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError> {
        // Create a query for detailed retrieval
        let query = GetPolicyQuery {
            policy_id: policy_id.clone(),
            include_versions: true,
        };

        match self.storage.find_by_id_with_versions(policy_id).await {
            Ok(policy) => Ok(policy),
            Err(e) => Err(e),
        }
    }
}
