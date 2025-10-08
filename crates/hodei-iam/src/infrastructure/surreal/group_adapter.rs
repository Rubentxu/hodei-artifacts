//! SurrealDB adapter for Group persistence operations

use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use tracing::{debug, error, info};

// Import the ports from features
use crate::features::create_group::ports::CreateGroupPort;
use crate::features::add_user_to_group::ports::GroupFinder;
use crate::features::get_effective_policies::ports::GroupFinderPort;

// Import errors from features
use crate::features::create_group::error::CreateGroupError;
use crate::features::add_user_to_group::error::AddUserToGroupError;

// Import internal domain entities
use crate::internal::domain::Group;

/// SurrealDB adapter for Group persistence operations
pub struct SurrealGroupAdapter {
    db: Arc<Surreal<Any>>,
}

impl SurrealGroupAdapter {
    /// Create a new SurrealGroupAdapter
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreateGroupPort for SurrealGroupAdapter {
    async fn save_group(&self, group: &Group) -> Result<(), CreateGroupError> {
        info!("Saving group with HRN: {}", group.hrn);
        
        let group_table = "group";
        let group_id = group.hrn.resource_id();
        
        let created: Result<Option<Group>, surrealdb::Error> = self.db
            .create((group_table, group_id))
            .content(group.clone())
            .await;
            
        match created {
            Ok(Some(_)) => {
                info!("Group saved successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to save group - no group returned");
                Err(CreateGroupError::PersistenceError("Failed to save group".to_string()))
            }
            Err(e) => {
                error!("Database error while saving group: {}", e);
                Err(CreateGroupError::PersistenceError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl GroupFinder for SurrealGroupAdapter {
    async fn find_group_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, AddUserToGroupError> {
        debug!("Finding group by HRN: {}", hrn);
        
        let group_table = "group";
        let group_id = hrn.resource_id();
        
        let group: Result<Option<Group>, surrealdb::Error> = self.db
            .select((group_table, group_id))
            .await;
            
        match group {
            Ok(group) => {
                if group.is_some() {
                    info!("Group found");
                } else {
                    info!("Group not found");
                }
                Ok(group)
            }
            Err(e) => {
                error!("Database error while finding group: {}", e);
                Err(AddUserToGroupError::PersistenceError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl GroupFinderPort for SurrealGroupAdapter {
    async fn find_groups_by_user_hrn(
        &self,
        user_hrn: &Hrn,
    ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>> {
        info!("Finding groups for user: {}", user_hrn);
        
        // This is a graph query in SurrealDB - find all groups where the user is a member
        let query = "SELECT * FROM group WHERE $user_hrn IN group_hrns";
        
        let mut result = self.db
            .query(query)
            .bind(("user_hrn", user_hrn.to_string()))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
        let groups: Vec<Group> = result
            .take(0)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
        info!("Found {} groups for user", groups.len());
        Ok(groups)
    }
}
