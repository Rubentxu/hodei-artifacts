use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::dto::GetEffectiveScpsCommand;
use crate::features::get_effective_scps::di::get_effective_scps_use_case;
use crate::shared::infrastructure::surreal::{SurrealScpRepository, SurrealAccountRepository, SurrealOuRepository};
use hodei_authorizer::features::evaluate_permissions::ports::OrganizationBoundaryProvider;
use hodei_authorizer::features::evaluate_permissions::error::EvaluatePermissionsError;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::str::FromStr;

/// SurrealDB implementation of OrganizationBoundaryProvider
pub struct SurrealOrganizationBoundaryProvider {
    db: Surreal<Any>,
}

impl SurrealOrganizationBoundaryProvider {
    /// Create a new SurrealOrganizationBoundaryProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    /// Get effective SCPs for a resource account
    async fn get_effective_scps_for(&self, resource_hrn: &Hrn) -> Result<PolicySet, EvaluatePermissionsError> {
        // Create repositories
        let scp_repository = SurrealScpRepository::new(self.db.clone());
        let account_repository = SurrealAccountRepository::new(self.db.clone());
        let ou_repository = SurrealOuRepository::new(self.db.clone());
        
        // Create use case
        let use_case = get_effective_scps_use_case(scp_repository, account_repository, ou_repository);
        
        // Create command
        let command = GetEffectiveScpsCommand {
            target_hrn: resource_hrn.to_string(),
        };
        
        // Execute use case
        let result = use_case.execute(command).await
            .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?;
        
        // Create a new PolicySet for Cedar
        let mut policy_set = PolicySet::new();
        
        // Add each SCP policy to the PolicySet
        for scp_hrn_string in result.effective_scps {
            let scp_hrn = Hrn::from_str(&scp_hrn_string)
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?;
            
            // Find the actual SCP object
            let scp_repository = SurrealScpRepository::new(self.db.clone());
            let scp = scp_repository.find_by_hrn(&scp_hrn).await
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?
                .ok_or_else(|| EvaluatePermissionsError::OrganizationBoundaryProvider(format!("SCP not found: {}", scp_hrn_string)))?;
            
            // Parse the SCP policy text and add to PolicySet
            let policy = cedar_policy::Policy::from_str(&scp.policy_text)
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(format!("Failed to parse SCP policy: {}", e)))?;
            
            policy_set.add_policy(policy);
        }
        
        Ok(policy_set)
    }
}
