//! In-memory adapter for Group persistence operations
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import ports from features
use crate::features::create_group::ports::CreateGroupPort;
use crate::features::add_user_to_group::ports::GroupFinder;

// Import errors from features
use crate::features::create_group::error::CreateGroupError;
use crate::features::add_user_to_group::error::AddUserToGroupError;

// Import internal domain entities
use crate::internal::domain::Group;

/// In-memory adapter for Group operations
pub struct InMemoryGroupAdapter {
    store: RwLock<HashMap<String, Group>>,
}

impl InMemoryGroupAdapter {
    /// Create a new InMemoryGroupAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Check if group exists by HRN
    fn exists(&self, hrn: &Hrn) -> bool {
        let resource_id = hrn.resource_id();
        self.store.read().unwrap().contains_key(&resource_id)
    }
}

#[async_trait]
impl CreateGroupPort for InMemoryGroupAdapter {
    async fn save_group(&self, group: &Group) -> Result<(), CreateGroupError> {
        info!("Saving group with HRN: {}", group.hrn);
        
        let resource_id = group.hrn.resource_id();
        
        // Check uniqueness
        if self.exists(&group.hrn) {
            return Err(CreateGroupError::GroupAlreadyExists(group.hrn.to_string()));
        }

        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while writing group");
            CreateGroupError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        guard.insert(resource_id, group.clone());
        info!("Group saved successfully");
        Ok(())
    }
}

#[async_trait]
impl GroupFinder for InMemoryGroupAdapter {
    async fn find_group_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, AddUserToGroupError> {
        debug!("Finding group by HRN: {}", hrn);
        
        let resource_id = hrn.resource_id();
        let guard = self.store.read().unwrap();
        let group = guard.get(&resource_id).cloned();
        
        if group.is_some() {
            info!("Group found");
        } else {
            info!("Group not found");
        }
        
        Ok(group)
    }
}
