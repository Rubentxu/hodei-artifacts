//! In-memory adapter for User persistence operations
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import ports from features
use crate::features::create_user::ports::CreateUserPort;
use crate::features::add_user_to_group::ports::UserFinder;
use crate::features::add_user_to_group::ports::UserGroupPersister;

// Import errors from features
use crate::features::create_user::error::CreateUserError;
use crate::features::add_user_to_group::error::AddUserToGroupError;

// Import internal domain entities
use crate::internal::domain::User;

/// In-memory adapter for User operations
pub struct InMemoryUserAdapter {
    store: RwLock<HashMap<String, User>>,
}

impl InMemoryUserAdapter {
    /// Create a new InMemoryUserAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Check if user exists by HRN
    fn exists(&self, hrn: &Hrn) -> bool {
        let resource_id = hrn.resource_id();
        self.store.read().unwrap().contains_key(&resource_id)
    }
}

#[async_trait]
impl CreateUserPort for InMemoryUserAdapter {
    async fn save_user(&self, user: &User) -> Result<(), CreateUserError> {
        info!("Saving user with HRN: {}", user.hrn);
        
        let resource_id = user.hrn.resource_id();
        
        // Check uniqueness
        if self.exists(&user.hrn) {
            return Err(CreateUserError::UserAlreadyExists(user.hrn.to_string()));
        }

        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while writing user");
            CreateUserError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        guard.insert(resource_id, user.clone());
        info!("User saved successfully");
        Ok(())
    }
}

#[async_trait]
impl UserFinder for InMemoryUserAdapter {
    async fn find_user_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, AddUserToGroupError> {
        debug!("Finding user by HRN: {}", hrn);
        
        let resource_id = hrn.resource_id();
        let guard = self.store.read().unwrap();
        let user = guard.get(&resource_id).cloned();
        
        if user.is_some() {
            info!("User found");
        } else {
            info!("User not found");
        }
        
        Ok(user)
    }
}

#[async_trait]
impl UserGroupPersister for InMemoryUserAdapter {
    async fn save_user(&self, user: &User) -> Result<(), AddUserToGroupError> {
        info!("Updating user with HRN: {}", user.hrn);
        
        let resource_id = user.hrn.resource_id();
        
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while updating user");
            AddUserToGroupError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        if !guard.contains_key(&resource_id) {
            return Err(AddUserToGroupError::UserNotFound(user.hrn.to_string()));
        }

        guard.insert(resource_id, user.clone());
        info!("User updated successfully");
        Ok(())
    }
}
