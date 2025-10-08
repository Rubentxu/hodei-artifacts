use crate::internal::domain::Group;
use async_trait::async_trait;
use kernel::Hrn;
use super::error::CreateGroupError;

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
    /// * `group` - The group entity to save
    ///
    /// # Returns
    /// * `Ok(())` if the group was saved successfully
    /// * `Err(CreateGroupError)` if there was an error saving the group
    async fn save_group(&self, group: &Group) -> Result<(), CreateGroupError>;
}

/// Port for generating HRNs
///
/// This port abstracts HRN generation, allowing different implementations
/// (e.g., UUID-based, sequential, etc.)
pub trait HrnGenerator: Send + Sync {
    /// Generate a new HRN for a group
    ///
    /// # Arguments
    /// * `name` - The name of the group (used for HRN generation)
    ///
    /// # Returns
    /// * A new HRN for the group
    fn new_group_hrn(&self, name: &str) -> Hrn;
}