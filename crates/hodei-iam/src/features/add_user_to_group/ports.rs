use super::dto::{AddUserToGroupCommand, GroupLookupDto, UserLookupDto, UserPersistenceDto};
use super::error::AddUserToGroupError;
use async_trait::async_trait;
use kernel::Hrn;

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
    /// * `Ok(Some(UserLookupDto))` if the user was found
    /// * `Ok(None)` if no user with that HRN exists
    /// * `Err(AddUserToGroupError)` if there was an error during lookup
    async fn find_user_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<UserLookupDto>, AddUserToGroupError>;
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
    /// * `Ok(Some(GroupLookupDto))` if the group was found
    /// * `Ok(None)` if no group with that HRN exists
    /// * `Err(AddUserToGroupError)` if there was an error during lookup
    async fn find_group_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<GroupLookupDto>, AddUserToGroupError>;
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
    /// * `user_dto` - The user data transfer object to save
    ///
    /// # Returns
    /// * `Ok(())` if the user was saved successfully
    /// * `Err(AddUserToGroupError)` if there was an error saving the user
    async fn save_user(&self, user_dto: &UserPersistenceDto) -> Result<(), AddUserToGroupError>;
}

/// Port for the AddUserToGroup use case
///
/// This port defines the contract for executing the add user to group use case.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the execute method needed by external callers.
#[async_trait]
pub trait AddUserToGroupUseCasePort: Send + Sync {
    /// Execute the add user to group use case
    ///
    /// # Arguments
    /// * `command` - The add user to group command containing user and group details
    ///
    /// # Returns
    /// * `Ok(())` if the user was added to the group successfully
    /// * `Err(AddUserToGroupError)` if there was an error adding the user to the group
    async fn execute(&self, command: AddUserToGroupCommand) -> Result<(), AddUserToGroupError>;
}
