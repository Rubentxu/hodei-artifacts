//! In-memory adapter for UpdatePolicyPort
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import the port trait
use crate::features::update_policy::ports::UpdatePolicyPort;
use crate::features::update_policy::dto::{UpdatePolicyCommand, PolicyView};
use crate::features::update_policy::error::UpdatePolicyError;

/// In-memory adapter for UpdatePolicyPort
pub struct InMemoryUpdatePolicyAdapter {
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryUpdatePolicyAdapter {
    /// Create a new InMemoryUpdatePolicyAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Add a policy to the store (for testing purposes)
    pub fn add_policy(&self, policy_id: String, content: String, description: Option<String>) {
        let policy_id_obj = PolicyId::new(policy_id.clone());
        let policy = HodeiPolicy::new(policy_id_obj, content);
        
        let mut guard = self.store.write().unwrap();
        guard.insert(policy_id, policy);
    }

    /// Get a policy from the store (for testing purposes)
    pub fn get_policy(&self, policy_id: &str) -> Option<HodeiPolicy> {
        let guard = self.store.read().unwrap();
        guard.get(policy_id).cloned()
    }
}

#[async_trait]
impl UpdatePolicyPort for InMemoryUpdatePolicyAdapter {
    async fn update(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
        info!("Updating policy with ID: {}", command.policy_id);
        
        if !command.has_updates() {
            return Err(UpdatePolicyError::NoUpdatesProvided);
        }

        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while updating policy");
            UpdatePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        let existing_policy = guard.get_mut(&command.policy_id)
            .ok_or_else(|| UpdatePolicyError::PolicyNotFound(command.policy_id.clone()))?;

        // Update content if provided
        if let Some(new_content) = command.policy_content {
            let policy_id = PolicyId::new(command.policy_id.clone());
            let updated_policy = HodeiPolicy::new(policy_id, new_content);
            *existing_policy = updated_policy;
        }

        // For now, we don't handle description updates in the in-memory adapter
        // since HodeiPolicy doesn't have a description field in the kernel

        let policy = guard.get(&command.policy_id).unwrap().clone();
        
        let view = PolicyView {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Policy".to_string(),
                command.policy_id.clone(),
            ),
            name: command.policy_id.clone(),
            content: policy.content().to_string(),
            description: None, // HodeiPolicy from kernel doesn't have description
        };

        info!("Policy updated successfully");
        Ok(view)
    }
}
