use super::dto::GroupPersistenceDto;
use super::error::CreateGroupError;
use crate::infrastructure::hrn_generator::HrnGenerator;
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
