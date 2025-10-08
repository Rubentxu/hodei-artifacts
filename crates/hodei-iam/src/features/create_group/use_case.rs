use super::dto::{CreateGroupCommand, GroupView};
use super::error::CreateGroupError;
use super::ports::{CreateGroupPort, HrnGenerator};
use crate::internal::domain::Group;
use std::sync::Arc;

/// Use case for creating a new group
///
/// This use case orchestrates the group creation process:
/// 1. Generates a new HRN for the group
/// 2. Creates a Group entity
/// 3. Persists the group through the port
/// 4. Returns a GroupView DTO
pub struct CreateGroupUseCase<P: CreateGroupPort, G: HrnGenerator> {
    persister: Arc<P>,
    hrn_generator: Arc<G>,
}

impl<P: CreateGroupPort, G: HrnGenerator> CreateGroupUseCase<P, G> {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    /// * `persister` - Implementation of CreateGroupPort for persistence
    /// * `hrn_generator` - Implementation of HrnGenerator for HRN generation
    pub fn new(persister: Arc<P>, hrn_generator: Arc<G>) -> Self {
        Self {
            persister,
            hrn_generator,
        }
    }

    /// Execute the create group use case
    ///
    /// # Arguments
    /// * `cmd` - CreateGroupCommand containing group details
    ///
    /// # Returns
    /// * Ok(GroupView) if the group was created successfully
    /// * Err(CreateGroupError) if there was an error
    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, CreateGroupError> {
        // Generate a unique HRN using the HRN generator
        let hrn = self.hrn_generator.new_group_hrn(&cmd.group_name);
        
        // Create the group domain entity
        let group = Group::new(hrn.clone(), cmd.group_name, None);
        
        // Persist the group
        self.persister.save_group(&group).await?;
        
        // Return the view
        Ok(GroupView {
            hrn: hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}