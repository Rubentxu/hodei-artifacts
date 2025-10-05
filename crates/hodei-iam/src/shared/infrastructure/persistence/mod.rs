use crate::shared::application::ports::{
    GroupRepository, GroupRepositoryError, UserRepository, UserRepositoryError,
};
use crate::shared::domain::{Group, User};
/// In-memory repository implementations for testing
///
/// These repositories store data in memory using RwLock for thread-safe access
use async_trait::async_trait;
use kernel::Hrn;
use std::sync::RwLock;

/// In-memory implementation of UserRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: RwLock<Vec<User>>,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn save(&self, user: &User) -> Result<(), UserRepositoryError> {
        let mut users = self.users.write().map_err(|e| {
            UserRepositoryError::InternalError(format!("Failed to acquire write lock: {}", e))
        })?;

        // Remove existing user with same HRN if present
        users.retain(|u| u.hrn != user.hrn);

        // Add the new/updated user
        users.push(user.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
        let users = self.users.read().map_err(|e| {
            UserRepositoryError::InternalError(format!("Failed to acquire read lock: {}", e))
        })?;
        Ok(users.iter().find(|u| &u.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<User>, UserRepositoryError> {
        let users = self.users.read().map_err(|e| {
            UserRepositoryError::InternalError(format!("Failed to acquire read lock: {}", e))
        })?;
        Ok(users.clone())
    }
}

/// In-memory implementation of GroupRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryGroupRepository {
    groups: RwLock<Vec<Group>>,
}

impl InMemoryGroupRepository {
    pub fn new() -> Self {
        Self {
            groups: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GroupRepository for InMemoryGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), GroupRepositoryError> {
        let mut groups = self.groups.write().map_err(|e| {
            GroupRepositoryError::InternalError(format!("Failed to acquire write lock: {}", e))
        })?;

        // Remove existing group with same HRN if present
        groups.retain(|g| g.hrn != group.hrn);

        // Add the new/updated group
        groups.push(group.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, GroupRepositoryError> {
        let groups = self.groups.read().map_err(|e| {
            GroupRepositoryError::InternalError(format!("Failed to acquire read lock: {}", e))
        })?;
        Ok(groups.iter().find(|g| &g.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<Group>, GroupRepositoryError> {
        let groups = self.groups.read().map_err(|e| {
            GroupRepositoryError::InternalError(format!("Failed to acquire read lock: {}", e))
        })?;
        Ok(groups.clone())
    }
}
