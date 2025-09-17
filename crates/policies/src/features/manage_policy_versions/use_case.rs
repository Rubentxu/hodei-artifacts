use async_trait::async_trait;
use chrono::Utc;
use shared::hrn::{Hrn, UserId};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::dto::{CreatePolicyVersionCommand, CreatePolicyVersionResponse, GetPolicyVersionsQuery, RollbackPolicyVersionCommand, RollbackPolicyVersionResponse};
use super::error::ManagePolicyVersionsError;
use super::ports::{PolicyVersionAuditor, PolicyVersionHistory, PolicyVersionManager, PolicyVersionStorage, PolicyVersionValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};

/// Use case for managing policy versions
pub struct ManagePolicyVersionsUseCase<PVV, PVH, PVS, PVA> {
    validator: Arc<PVV>,
    history: Arc<PVH>,
    storage: Arc<PVS>,
    auditor: Arc<PVA>,
    _phantom: PhantomData<PVV>,
    _phantom2: PhantomData<PVH>,
    _phantom3: PhantomData<PVS>,
    _phantom4: PhantomData<PVA>,
}

impl<PVV, PVH, PVS, PVA> ManagePolicyVersionsUseCase<PVV, PVH, PVS, PVA> {
    pub fn new(
        validator: Arc<PVV>,
        history: Arc<PVH>,
        storage: Arc<PVS>,
        auditor: Arc<PVA>,
    ) -> Self {
        Self {
            validator,
            history,
            storage,
            auditor,
            _phantom: PhantomData,
            _phantom2: PhantomData,
            _phantom3: PhantomData,
            _phantom4: PhantomData,
        }
    }

    /// Create a new policy version
    pub async fn create_version(&self, command: CreatePolicyVersionCommand) -> Result<CreatePolicyVersionResponse, ManagePolicyVersionsError> {
        info!("Creating new version for policy: {}", command.policy_id);

        // Validate content
        self.validator.validate_version_content(&command.content).await?;

        // Get the latest version number and increment
        let existing_versions = self.storage.find_versions_by_policy(&command.policy_id).await?;
        let next_version = existing_versions.iter()
            .map(|v| v.version)
            .max()
            .unwrap_or(0) + 1;

        // Validate version number
        self.validator.validate_version_number(next_version).await?;

        // Create version
        let version = PolicyVersion {
            id: Hrn::new(&format!("{}/versions/{}", command.policy_id, next_version)).unwrap(),
            policy_id: command.policy_id.clone(),
            version: next_version,
            content: command.content.clone(),
            created_at: Utc::now(),
            created_by: command.created_by.clone(),
        };

        // Save version
        self.storage.save_version(&version).await?;

        // Log the creation
        self.auditor.log_version_creation(&command.policy_id, next_version, &command.created_by).await?;

        info!("Version {} created successfully for policy: {}", next_version, command.policy_id);

        Ok(CreatePolicyVersionResponse {
            policy_id: command.policy_id,
            version: next_version,
            content: command.content,
            created_at: version.created_at.to_rfc3339(),
            created_by: command.created_by,
        })
    }

    /// Get all versions for a policy
    pub async fn get_versions(&self, query: GetPolicyVersionsQuery) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError> {
        info!("Getting versions for policy: {}", query.policy_id);

        self.history.get_version_history(&query.policy_id, query.limit).await
    }

    /// Rollback policy to specific version
    pub async fn rollback_to_version(&self, command: RollbackPolicyVersionCommand) -> Result<RollbackPolicyVersionResponse, ManagePolicyVersionsError> {
        info!("Rolling back policy {} to version {}", command.policy_id, command.target_version);

        // Get current policy to validate rollback
        // This would need integration with the main policy storage
        // For now, simplified implementation

        // Validate rollback is allowed
        // self.validator.validate_rollback_allowed(&policy, command.target_version, &command.rollback_by).await?;

        // Get the target version
        let target_version = self.storage.find_version(&command.policy_id, command.target_version).await?
            .ok_or_else(|| ManagePolicyVersionsError::version_not_found(command.target_version))?;

        // Update current version
        self.storage.update_current_version(&command.policy_id, command.target_version).await?;

        // Log the rollback
        let current_version = 0; // Would need to get actual current version
        self.auditor.log_version_rollback(&command.policy_id, current_version, command.target_version, &command.rollback_by).await?;

        info!("Policy {} rolled back to version {}", command.policy_id, command.target_version);

        Ok(RollbackPolicyVersionResponse {
            policy_id: command.policy_id,
            from_version: current_version,
            to_version: command.target_version,
            rolled_back_at: Utc::now().to_rfc3339(),
            rolled_back_by: command.rollback_by,
            success: true,
            message: format!("Successfully rolled back to version {}", command.target_version),
        })
    }

    /// Get version history with detailed information
    pub async fn get_version_history(&self, policy_id: &PolicyId, limit: Option<usize>) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError> {
        info!("Getting version history for policy: {}", policy_id);

        self.history.get_version_history(policy_id, limit).await
    }

    /// Compare two versions
    pub async fn compare_versions(&self, policy_id: &PolicyId, from_version: i64, to_version: i64) -> Result<String, ManagePolicyVersionsError> {
        info!("Comparing versions {} and {} for policy: {}", from_version, to_version, policy_id);

        self.history.get_version_diff(policy_id, from_version, to_version).await
    }
}

#[async_trait]
impl<PVV, PVH, PVS, PVA> PolicyVersionManager for ManagePolicyVersionsUseCase<PVV, PVH, PVS, PVA>
where
    PVV: PolicyVersionValidator + Send + Sync,
    PVH: PolicyVersionHistory + Send + Sync,
    PVS: PolicyVersionStorage + Send + Sync,
    PVA: PolicyVersionAuditor + Send + Sync,
{
    async fn create_version(&self, policy_id: &PolicyId, content: String, user_id: &UserId) -> Result<PolicyVersion, ManagePolicyVersionsError> {
        let command = CreatePolicyVersionCommand {
            policy_id: policy_id.clone(),
            content,
            created_by: user_id.clone(),
        };

        let response = self.create_version(command).await?;

        // Convert response back to PolicyVersion
        Ok(PolicyVersion {
            id: Hrn::new(&format!("{}/versions/{}", response.policy_id, response.version)).unwrap(),
            policy_id: response.policy_id,
            version: response.version,
            content: response.content,
            created_at: chrono::DateTime::parse_from_rfc3339(&response.created_at)
                .map_err(|_| ManagePolicyVersionsError::storage_error("Invalid timestamp"))?
                .with_timezone(&Utc),
            created_by: response.created_by,
        })
    }

    async fn get_versions(&self, policy_id: &PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError> {
        let query = GetPolicyVersionsQuery {
            policy_id: policy_id.clone(),
            limit: None,
            offset: None,
        };

        self.get_versions(query).await
    }

    async fn get_version(&self, policy_id: &PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError> {
        self.storage.find_version(policy_id, version).await
    }

    async fn rollback_to_version(&self, policy_id: &PolicyId, version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError> {
        let command = RollbackPolicyVersionCommand {
            policy_id: policy_id.clone(),
            target_version: version,
            rollback_by: user_id.clone(),
            reason: None,
        };

        let response = self.rollback_to_version(command).await?;
        if response.success {
            Ok(())
        } else {
            Err(ManagePolicyVersionsError::cannot_rollback("Rollback failed"))
        }
    }
}
