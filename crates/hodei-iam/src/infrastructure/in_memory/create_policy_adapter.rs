//! In-memory adapter for CreatePolicyPort
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import the port trait
use crate::features::create_policy::ports::{CreatePolicyPort, PolicyValidator, ValidationResult, PolicyValidationError};
use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::create_policy::error::CreatePolicyError;

/// In-memory adapter for CreatePolicyPort
pub struct InMemoryCreatePolicyAdapter {
    account_id: String,
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryCreatePolicyAdapter {
    /// Create a new InMemoryCreatePolicyAdapter
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Build an HRN for the given policy id
    fn build_hrn(&self, policy_id: &str) -> Hrn {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            self.account_id.clone(),
            "policy".to_string(),
            policy_id.to_string(),
        )
    }

    /// Check if policy exists by ID
    fn exists(&self, policy_id: &str) -> bool {
        self.store.read().unwrap().contains_key(policy_id)
    }
}

#[async_trait]
impl CreatePolicyPort for InMemoryCreatePolicyAdapter {
    async fn create(&self, command: CreatePolicyCommand) -> Result<HodeiPolicy, CreatePolicyError> {
        info!("Creating policy with ID: {}", command.policy_id);
        
        // Basic input validation
        if command.policy_id.trim().is_empty() {
            return Err(CreatePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        // Check uniqueness
        if self.exists(&command.policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists(
                command.policy_id.clone(),
            ));
        }

        // Build policy entity
        let policy_id = PolicyId::new(command.policy_id.clone());
        let policy = HodeiPolicy::new(policy_id, command.policy_content);

        // Store the policy
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while creating policy");
            CreatePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        guard.insert(command.policy_id, policy.clone());
        info!("Policy created successfully");
        Ok(policy)
    }
}
