//! SurrealDB adapter for User persistence operations

use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use tracing::{debug, error, info};

// Import the ports from features
use crate::features::create_user::ports::CreateUserPort;
use crate::features::add_user_to_group::ports::UserFinder;
use crate::features::add_user_to_group::ports::UserGroupPersister;
use crate::features::get_effective_policies::ports::UserFinderPort;

// Import errors from features
use crate::features::create_user::error::CreateUserError;
use crate::features::add_user_to_group::error::AddUserToGroupError;

// Import internal domain entities
use crate::internal::domain::User;

/// SurrealDB adapter for User persistence operations
pub struct SurrealUserAdapter {
    db: Arc<Surreal<Any>>,
}

impl SurrealUserAdapter {
    /// Create a new SurrealUserAdapter
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreateUserPort for SurrealUserAdapter {
    async fn save_user(&self, user: &User) -> Result<(), CreateUserError> {
        info!("Saving user with HRN: {}", user.hrn);
        
        let user_table = "user";
        let user_id = user.hrn.resource_id();
        
        let created: Result<Option<User>, surrealdb::Error> = self.db
            .create((user_table, user_id))
            .content(user.clone())
            .await;
            
        match created {
            Ok(Some(_)) => {
                info!("User saved successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to save user - no user returned");
                Err(CreateUserError::PersistenceError("Failed to save user".to_string()))
            }
            Err(e) => {
                error!("Database error while saving user: {}", e);
                Err(CreateUserError::PersistenceError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl UserFinder for SurrealUserAdapter {
    async fn find_user_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, AddUserToGroupError> {
        debug!("Finding user by HRN: {}", hrn);
        
        let user_table = "user";
        let user_id = hrn.resource_id();
        
        let user: Result<Option<User>, surrealdb::Error> = self.db
            .select((user_table, user_id))
            .await;
            
        match user {
            Ok(user) => {
                if user.is_some() {
                    info!("User found");
                } else {
                    info!("User not found");
                }
                Ok(user)
            }
            Err(e) => {
                error!("Database error while finding user: {}", e);
                Err(AddUserToGroupError::PersistenceError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl UserGroupPersister for SurrealUserAdapter {
    async fn save_user(&self, user: &User) -> Result<(), AddUserToGroupError> {
        info!("Updating user with HRN: {}", user.hrn);
        
        let user_table = "user";
        let user_id = user.hrn.resource_id();
        
        let updated: Result<Option<User>, surrealdb::Error> = self.db
            .update((user_table, user_id))
            .content(user.clone())
            .await;
            
        match updated {
            Ok(Some(_)) => {
                info!("User updated successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to update user - user not found");
                Err(AddUserToGroupError::PersistenceError("User not found".to_string()))
            }
            Err(e) => {
                error!("Database error while updating user: {}", e);
                Err(AddUserToGroupError::PersistenceError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl UserFinderPort for SurrealUserAdapter {
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Finding user by HRN for policy lookup: {}", hrn);
        
        let user_table = "user";
        let user_id = hrn.resource_id();
        
        let user: Result<Option<User>, surrealdb::Error> = self.db
            .select((user_table, user_id))
            .await;
            
        match user {
            Ok(user) => {
                if user.is_some() {
                    info!("User found for policy lookup");
                } else {
                    info!("User not found for policy lookup");
                }
                Ok(user)
            }
            Err(e) => {
                error!("Database error while finding user for policy lookup: {}", e);
                Err(Box::new(e))
            }
        }
    }
}
