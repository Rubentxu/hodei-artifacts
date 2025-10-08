//! In-memory adapter for DeletePolicyPort
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import the port trait
use crate::features::delete_policy::ports::DeletePolicyPort;
use crate::features::delete_policy::dto::DeletePolicyCommand;
use crate::features::delete_policy::error::DeletePolicyError;

/// In-memory adapter for DeletePolicyPort
pub struct InMemoryDeletePolicyAdapter {
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryDeletePolicyAdapter {
    /// Create a new InMemoryDeletePolicyAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new InMemoryDeletePolicyAdapter with existing policies
    pub fn with_existing_policies(policy_ids: Vec<String>) -> Self {
        let mut store = HashMap::new();
        for policy_id in policy_ids {
            let policy_id_obj = PolicyId::new(policy_id.clone());
            let policy = HodeiPolicy::new(policy_id_obj, "permit(principal, action, resource);".to_string());
            store.insert(policy_id, policy);
        }
        
        Self {
            store: RwLock::new(store),
        }
    }

    /// Add a policy to the store (for testing purposes)
    pub fn add_policy(&self, policy_id: String, content: String) {
        let policy_id_obj = PolicyId::new(policy_id.clone());
        let policy = HodeiPolicy::new(policy_id_obj, content);
        
        let mut guard = self.store.write().unwrap();
        guard.insert(policy_id, policy);
    }
}

#[async_trait]
impl DeletePolicyPort for InMemoryDeletePolicyAdapter {
    async fn delete(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        info!("Deleting policy with ID: {}", command.policy_id);
        
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while deleting policy");
            DeletePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        if guard.remove(&command.policy_id).is_none() {
            return Err(DeletePolicyError::PolicyNotFound(command.policy_id.clone()));
        }

        info!("Policy deleted successfully");
        Ok(())
    }
}
