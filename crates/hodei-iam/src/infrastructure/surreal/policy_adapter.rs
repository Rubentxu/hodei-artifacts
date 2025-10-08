//! SurrealDB adapter for Policy persistence operations

use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use tracing::{debug, error, info};

// Import the ports from features
use crate::features::create_policy::ports::CreatePolicyPort;
use crate::features::get_effective_policies::ports::PolicyFinderPort;

// Import DTOs and errors from features
use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::create_policy::error::CreatePolicyError;

// Import internal domain entities
use crate::internal::domain::User;
use crate::internal::domain::Group;

// Import kernel policy types
use kernel::domain::policy::{HodeiPolicy, PolicyId};

/// SurrealDB adapter for Policy persistence operations
pub struct SurrealPolicyAdapter {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyAdapter {
    /// Create a new SurrealPolicyAdapter
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreatePolicyPort for SurrealPolicyAdapter {
    async fn create(&self, command: CreatePolicyCommand) -> Result<HodeiPolicy, CreatePolicyError> {
        info!("Creating policy with ID: {}", command.policy_id);
        
        // Create HRN for the policy
        let policy_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(), // This should come from context
            "Policy".to_string(),
            command.policy_id.clone(),
        );
        
        // Create the policy entity
        let policy_id = PolicyId::new(command.policy_id.clone());
        let policy: HodeiPolicy = HodeiPolicy::new(policy_id, command.policy_content);
        
        let policy_table = "policy";
        let policy_id = policy_hrn.resource_id();
        
        let created: Result<Option<HodeiPolicy>, surrealdb::Error> = self.db
            .create((policy_table, policy_id))
            .content(policy.clone())
            .await;
            
        match created {
            Ok(Some(created_policy)) => {
                info!("Policy created successfully");
                Ok(created_policy)
            }
            Ok(None) => {
                error!("Failed to create policy - no policy returned");
                Err(CreatePolicyError::StorageError("Failed to create policy".to_string()))
            }
            Err(e) => {
                error!("Database error while creating policy: {}", e);
                Err(CreatePolicyError::StorageError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl PolicyFinderPort for SurrealPolicyAdapter {
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<HodeiPolicy>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Finding policies for principal: {}", principal_hrn);
        
        // This is a graph query in SurrealDB - find all policies attached to the principal
        let query = "SELECT * FROM policy WHERE $principal_hrn IN attached_principals";
        
        let mut result = self.db
            .query(query)
            .bind(("principal_hrn", principal_hrn.to_string()))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
        let policies: Vec<surrealdb::sql::Object> = result
            .take(0)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        // Convert surreal objects to HodeiPolicy
        let mut hodei_policies = Vec::new();
        for policy_obj in policies {
            // Extract and clean the id and content values separately to avoid borrowing issues
            let id_opt = policy_obj.get("id").map(|v| {
                let s = v.to_string();
                s.strip_prefix('\"').unwrap_or(&s).strip_suffix('\"').unwrap_or(&s).to_string()
            });
            let content_opt = policy_obj.get("content").map(|v| {
                let s = v.to_string();
                s.strip_prefix('\"').unwrap_or(&s).strip_suffix('\"').unwrap_or(&s).to_string()
            });
            
            if let (Some(id), Some(content)) = (id_opt, content_opt) {
                let policy_id = PolicyId::new(id);
                let hodei_policy = HodeiPolicy::new(policy_id, content);
                hodei_policies.push(hodei_policy);
            }
        }
        
        info!("Found {} policies for principal", hodei_policies.len());
        Ok(hodei_policies)
    }
}
