//! Mock implementations for testing
//!
//! This module provides mock implementations of the ports for use in unit tests.

use super::ports::{UserFinder, GroupFinder, UserGroupPersister};
use crate::internal::domain::{User, Group};
use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;

/// Mock implementation of UserFinder for testing
pub struct MockUserFinder {
    /// The user to return (None if not found)
    pub user: Option<User>,
    /// Whether the operation should fail
    pub should_fail: bool,
}

#[async_trait]
impl UserFinder for MockUserFinder {
    async fn find_user_by_hrn(&self, _hrn: &Hrn) -> Result<Option<User>, super::error::AddUserToGroupError> {
        if self.should_fail {
            Err(super::error::AddUserToGroupError::PersistenceError(
                "Mock failure".to_string(),
            ))
        } else {
            Ok(self.user.clone())
        }
    }
}

impl MockUserFinder {
    /// Create a new mock with a user
    pub fn with_user(user: User) -> Self {
        Self {
            user: Some(user),
            should_fail: false,
        }
    }

    /// Create a new mock with no user (not found)
    pub fn not_found() -> Self {
        Self {
            user: None,
            should_fail: false,
        }
    }

    /// Create a new mock that will fail
    pub fn failing() -> Self {
        Self {
            user: None,
            should_fail: true,
        }
    }
}

/// Mock implementation of GroupFinder for testing
pub struct MockGroupFinder {
    /// The group to return (None if not found)
    pub group: Option<Group>,
    /// Whether the operation should fail
    pub should_fail: bool,
}

#[async_trait]
impl GroupFinder for MockGroupFinder {
    async fn find_group_by_hrn(&self, _hrn: &Hrn) -> Result<Option<Group>, super::error::AddUserToGroupError> {
        if self.should_fail {
            Err(super::error::AddUserToGroupError::PersistenceError(
                "Mock failure".to_string(),
            ))
        } else {
            Ok(self.group.clone())
        }
    }
}

impl MockGroupFinder {
    /// Create a new mock with a group
    pub fn with_group(group: Group) -> Self {
        Self {
            group: Some(group),
            should_fail: false,
        }
    }

    /// Create a new mock with no group (not found)
    pub fn not_found() -> Self {
        Self {
            group: None,
            should_fail: false,
        }
    }

    /// Create a new mock that will fail
    pub fn failing() -> Self {
        Self {
            group: None,
            should_fail: true,
        }
    }
}

/// Mock implementation of UserGroupPersister for testing
pub struct MockUserGroupPersister {
    /// Whether the save operation should fail
    pub should_fail: bool,
    /// The user that was saved (for inspection in tests)
    pub saved_user: Option<User>,
}

#[async_trait]
impl UserGroupPersister for MockUserGroupPersister {
    async fn save_user(&self, user: &User) -> Result<(), super::error::AddUserToGroupError> {
        if self.should_fail {
            Err(super::error::AddUserToGroupError::PersistenceError(
                "Mock failure".to_string(),
            ))
        } else {
            // In a real mock, we might store the user for inspection
            // For this simple mock, we just return Ok
            Ok(())
        }
    }
}

impl MockUserGroupPersister {
    /// Create a new mock with default settings
    pub fn new() -> Self {
        Self {
            should_fail: false,
            saved_user: None,
        }
    }

    /// Create a new mock that will fail
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            saved_user: None,
        }
    }
}