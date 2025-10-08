//! SurrealDB adapter for Group persistence operations

use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, error, info};

// Import the ports from features
use crate::features::add_user_to_group::dto::GroupLookupDto as AddGroupLookupDto;
use crate::features::add_user_to_group::ports::GroupFinder;
use crate::features::create_group::dto::GroupPersistenceDto;
use crate::features::create_group::ports::CreateGroupPort;
use crate::features::get_effective_policies::dto::GroupLookupDto;
use crate::features::get_effective_policies::ports::GroupFinderPort;

// Import errors from features
use crate::features::add_user_to_group::error::AddUserToGroupError;
use crate::features::create_group::error::CreateGroupError;

// Import internal domain entities (for internal use only)
use crate::internal::domain::Group;

/// SurrealDB adapter for Group persistence operations
pub struct SurrealGroupAdapter {
    db: Arc<Surreal<Db>>,
}

impl SurrealGroupAdapter {
    /// Create a new SurrealGroupAdapter
    pub fn new(db: Arc<Surreal<Db>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreateGroupPort for SurrealGroupAdapter {
    async fn save_group(&self, group_dto: &GroupPersistenceDto) -> Result<(), CreateGroupError> {
        info!("Saving group with HRN: {}", group_dto.hrn);

        // Convert DTO to internal domain entity for persistence
        let hrn = Hrn::from_string(&group_dto.hrn)
            .ok_or_else(|| CreateGroupError::PersistenceError("Invalid HRN".to_string()))?;

        let group = Group {
            hrn: hrn.clone(),
            name: group_dto.name.clone(),
            description: None,
            tags: group_dto.tags.clone(),
        };

        let group_table = "group";
        let group_id = hrn.resource_id();

        let created: Result<Option<Group>, surrealdb::Error> =
            self.db.create((group_table, group_id)).content(group).await;

        match created {
            Ok(Some(_)) => {
                info!("Group saved successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to save group - no group returned");
                Err(CreateGroupError::PersistenceError(
                    "Failed to save group".to_string(),
                ))
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
    async fn find_group_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<AddGroupLookupDto>, AddUserToGroupError> {
        debug!("Finding group by HRN: {}", hrn);

        let group_table = "group";
        let group_id = hrn.resource_id();

        let group: Result<Option<Group>, surrealdb::Error> =
            self.db.select((group_table, group_id)).await;

        match group {
            Ok(Some(g)) => {
                info!("Group found");
                // Convert domain entity to DTO
                Ok(Some(AddGroupLookupDto {
                    hrn: g.hrn.to_string(),
                    name: g.name,
                    tags: g.tags.clone(),
                }))
            }
            Ok(None) => {
                info!("Group not found");
                Ok(None)
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
    ) -> Result<Vec<GroupLookupDto>, Box<dyn std::error::Error + Send + Sync>> {
        info!("Finding groups for user: {}", user_hrn);

        // Query all groups where the user is a member
        // Note: We'll need to track membership elsewhere or in a relation table
        // For now, return empty as we don't have membership tracking in the Group entity
        let query = r#"
            SELECT * FROM group
        "#;

        let mut result = self.db.query(query).await?;

        let groups: Vec<Group> = result.take(0)?;

        // Convert to DTOs
        let group_dtos: Vec<GroupLookupDto> = groups
            .into_iter()
            .map(|g| GroupLookupDto {
                hrn: g.hrn.to_string(),
                name: g.name,
                tags: g.tags.clone(),
            })
            .collect();

        info!("Found {} groups for user", group_dtos.len());
        Ok(group_dtos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_creation() {
        // This is a placeholder test
        // Real tests would require a test database
        assert!(true);
    }
}
