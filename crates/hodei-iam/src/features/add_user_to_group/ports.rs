use crate::internal::domain::{User, Group};
use async_trait::async_trait;
use kernel::Hrn;
use super::error::AddUserToGroupError;

/// Port for finding users by HRN
///
/// This port abstracts user lookup operations.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the operations needed by the add_user_to_group feature.
#[async_trait]
pub trait UserFinder: Send + Sync {
    /// Find a user by HRN
    ///
    /// # Arguments
    /// * `hrn` - The HRN of the user to find
    ///
    /// # Returns
    /// * `Ok(Some(User))` if the user was found
    /// * `Ok(None)` if no user with that HRN exists
    /// * `Err(AddUserToGroupError)` if there was an error during lookup
    async fn find_user_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, AddUserToGroupError>;
}

/// Port for finding groups by HRN
///
/// This port abstracts group lookup operations.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the operations needed by the add_user_to_group feature.
#[async_trait]
pub trait GroupFinder: Send + Sync {
    /// Find a group by HRN
    ///
    /// # Arguments
    /// * `hrn` - The HRN of the group to find
    ///
    /// # Returns
    /// * `Ok(Some(Group))` if the group was found
    /// * `Ok(None)` if no group with that HRN exists
    /// * `Err(AddUserToGroupError)` if there was an error during lookup
    async fn find_group_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, AddUserToGroupError>;
}

/// Port for persisting users
///
/// This port abstracts user persistence operations.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the operations needed by the add_user_to_group feature.
#[async_trait]
pub trait UserGroupPersister: Send + Sync {
    /// Save a user to the persistence layer
    ///
    /// # Arguments
    /// * `user` - The user entity to save
    ///
    /// # Returns
    /// * `Ok(())` if the user was saved successfully
    /// * `Err(AddUserToGroupError)` if there was an error saving the user
    async fn save_user(&self, user: &User) -> Result<(), AddUserToGroupError>;
}