use super::dto::{CreateGroupCommand, GroupPersistenceDto, GroupView};
use super::error::CreateGroupError;
use async_trait::async_trait;

/// Port for persisting groups
///
/// This port abstracts group persistence operations.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the operations needed by the create_group feature.
#[async_trait]
pub trait CreateGroupPort: Send + Sync {
    /// Save a group to the persistence layer
    ///
    /// # Arguments
    /// * `group_dto` - The group data transfer object to save
    ///
    /// # Returns
    /// * `Ok(())` if the group was saved successfully
    /// * `Err(CreateGroupError)` if there was an error saving the group
    async fn save_group(&self, group_dto: &GroupPersistenceDto) -> Result<(), CreateGroupError>;
}

/// Port for the CreateGroup use case
///
/// This port defines the contract for executing the create group use case.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the execute method needed by external callers.
#[async_trait]
pub trait CreateGroupUseCasePort: Send + Sync {
    /// Execute the create group use case
    ///
    /// # Arguments
    /// * `command` - The create group command containing group details
    ///
    /// # Returns
    /// * `Ok(GroupView)` if the group was created successfully
    /// * `Err(CreateGroupError)` if there was an error creating the group
    async fn execute(&self, command: CreateGroupCommand) -> Result<GroupView, CreateGroupError>;
}
