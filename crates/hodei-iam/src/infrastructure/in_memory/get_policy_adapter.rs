//! In-memory adapter for PolicyReader port
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{debug, info, warn};

// Import the port trait
use crate::features::get_policy::ports::PolicyReader;
use crate::features::get_policy::dto::{GetPolicyQuery, PolicyView};
use crate::features::get_policy::error::GetPolicyError;

/// In-memory adapter for PolicyReader port
pub struct InMemoryPolicyReaderAdapter {
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryPolicyReaderAdapter {
    /// Create a new InMemoryPolicyReaderAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
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
impl PolicyReader for InMemoryPolicyReaderAdapter {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        info!("Getting policy with HRN: {}", hrn);
        
        let policy_id = hrn.resource_id();
        let guard = self.store.read().unwrap();
        let policy = guard.get(policy_id)
            .ok_or_else(|| GetPolicyError::PolicyNotFound(policy_id.to_string()))?
            .clone();

        let view = PolicyView {
            hrn: hrn.clone(),
            name: policy_id.to_string(),
            content: policy.content().to_string(),
            description: None, // HodeiPolicy from kernel doesn't have description
        };

        info!("Policy retrieved successfully");
        Ok(view)
    }
}
