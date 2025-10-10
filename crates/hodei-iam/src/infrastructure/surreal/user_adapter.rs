//! SurrealDB adapter for User persistence operations

use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, error, info};

// Import the ports from features
use crate::features::add_user_to_group::dto::{
    UserLookupDto as AddUserLookupDto, UserPersistenceDto,
};
use crate::features::add_user_to_group::ports::{UserFinder, UserGroupPersister};
use crate::features::create_user::dto::UserPersistenceDto as CreateUserPersistenceDto;
use crate::features::create_user::ports::CreateUserPort;
use crate::features::get_effective_policies::dto::UserLookupDto;
use crate::features::get_effective_policies::ports::UserFinderPort;

// Import errors from features
use crate::features::add_user_to_group::error::AddUserToGroupError;
use crate::features::create_user::error::CreateUserError;
use crate::features::get_effective_policies::error::GetEffectivePoliciesError;

// Import internal domain entities (for internal use only)
use crate::internal::domain::User;

/// SurrealDB adapter for User persistence operations
pub struct SurrealUserAdapter {
    db: Arc<Surreal<Db>>,
}

impl SurrealUserAdapter {
    /// Create a new SurrealUserAdapter
    pub fn new(db: Arc<Surreal<Db>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreateUserPort for SurrealUserAdapter {
    async fn save_user(&self, user_dto: &CreateUserPersistenceDto) -> Result<(), CreateUserError> {
        info!("Saving user with HRN: {}", user_dto.hrn);

        // Convert DTO to internal domain entity for persistence
        let hrn = Hrn::from_string(&user_dto.hrn)
            .ok_or_else(|| CreateUserError::PersistenceError("Invalid HRN".to_string()))?;

        // Convert group HRN strings to Hrn objects
        let group_hrns: Vec<Hrn> = user_dto
            .group_hrns
            .iter()
            .filter_map(|hrn_str| Hrn::from_string(hrn_str))
            .collect();

        let user = User {
            hrn: hrn.clone(),
            name: user_dto.name.clone(),
            email: user_dto.email.clone(),
            group_hrns,
            tags: user_dto.tags.clone(),
        };

        let user_table = "user";
        let user_id = hrn.resource_id();

        let created: Result<Option<User>, surrealdb::Error> =
            self.db.create((user_table, user_id)).content(user).await;

        match created {
            Ok(Some(_)) => {
                info!("User saved successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to save user - no user returned");
                Err(CreateUserError::PersistenceError(
                    "Failed to save user".to_string(),
                ))
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
    async fn find_user_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<AddUserLookupDto>, AddUserToGroupError> {
        debug!("Finding user by HRN: {}", hrn);

        let user_table = "user";
        let user_id = hrn.resource_id();

        let user: Result<Option<User>, surrealdb::Error> =
            self.db.select((user_table, user_id)).await;

        match user {
            Ok(Some(u)) => {
                info!("User found");
                // Convert domain entity to DTO
                // Convert Hrn objects to strings
                let group_hrn_strings: Vec<String> =
                    u.group_hrns.iter().map(|hrn| hrn.to_string()).collect();

                Ok(Some(AddUserLookupDto {
                    hrn: u.hrn.to_string(),
                    name: u.name,
                    email: u.email,
                    group_hrns: group_hrn_strings,
                    tags: u.tags.clone(),
                }))
            }
            Ok(None) => {
                info!("User not found");
                Ok(None)
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
    async fn save_user(&self, user_dto: &UserPersistenceDto) -> Result<(), AddUserToGroupError> {
        info!("Updating user with HRN: {}", user_dto.hrn);

        // Convert DTO to internal domain entity for persistence
        let hrn = Hrn::from_string(&user_dto.hrn)
            .ok_or_else(|| AddUserToGroupError::PersistenceError("Invalid HRN".to_string()))?;

        // Convert group HRN strings to Hrn objects
        let group_hrns: Vec<Hrn> = user_dto
            .group_hrns
            .iter()
            .filter_map(|hrn_str| Hrn::from_string(hrn_str))
            .collect();

        let user = User {
            hrn: hrn.clone(),
            name: user_dto.name.clone(),
            email: user_dto.email.clone(),
            group_hrns,
            tags: user_dto.tags.clone(),
        };

        let user_table = "user";
        let user_id = hrn.resource_id();

        let updated: Result<Option<User>, surrealdb::Error> =
            self.db.update((user_table, user_id)).content(user).await;

        match updated {
            Ok(Some(_)) => {
                info!("User updated successfully");
                Ok(())
            }
            Ok(None) => {
                error!("Failed to update user - user not found");
                Err(AddUserToGroupError::PersistenceError(
                    "User not found".to_string(),
                ))
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
    ) -> Result<Option<UserLookupDto>, GetEffectivePoliciesError> {
        debug!("Finding user by HRN for policy lookup: {}", hrn);

        let user_table = "user";
        let user_id = hrn.resource_id();

        let user: Result<Option<User>, surrealdb::Error> =
            self.db.select((user_table, user_id)).await;

        match user {
            Ok(Some(u)) => {
                info!("User found for policy lookup");
                // Convert domain entity to DTO
                // Convert Hrn objects to strings
                let group_hrn_strings: Vec<String> =
                    u.group_hrns.iter().map(|hrn| hrn.to_string()).collect();

                Ok(Some(UserLookupDto {
                    hrn: u.hrn.to_string(),
                    name: u.name,
                    email: u.email,
                    group_hrns: group_hrn_strings,
                    tags: u.tags.clone(),
                }))
            }
            Ok(None) => {
                info!("User not found for policy lookup");
                Ok(None)
            }
            Err(e) => {
                error!("Database error while finding user for policy lookup: {}", e);
                Err(GetEffectivePoliciesError::RepositoryError(e.to_string()))
            }
        }
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
