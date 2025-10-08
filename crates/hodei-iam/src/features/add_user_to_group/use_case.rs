use super::dto::{AddUserToGroupCommand, UserPersistenceDto};
use super::error::AddUserToGroupError;
use super::ports::{GroupFinder, UserFinder, UserGroupPersister};
use kernel::Hrn;
use std::sync::Arc;

/// Use case for adding a user to a group
///
/// This use case orchestrates the process of adding a user to a group:
/// 1. Validates and parses the HRNs
/// 2. Finds the user and group
/// 3. Adds the user to the group
/// 4. Persists the updated user
pub struct AddUserToGroupUseCase<UF: UserFinder, GF: GroupFinder, UP: UserGroupPersister> {
    user_finder: Arc<UF>,
    group_finder: Arc<GF>,
    user_persister: Arc<UP>,
}

impl<UF: UserFinder, GF: GroupFinder, UP: UserGroupPersister> AddUserToGroupUseCase<UF, GF, UP> {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    /// * `user_finder` - Implementation of UserFinder for user lookup
    /// * `group_finder` - Implementation of GroupFinder for group lookup
    /// * `user_persister` - Implementation of UserGroupPersister for user persistence
    pub fn new(user_finder: Arc<UF>, group_finder: Arc<GF>, user_persister: Arc<UP>) -> Self {
        Self {
            user_finder,
            group_finder,
            user_persister,
        }
    }

    /// Execute the add user to group use case
    ///
    /// # Arguments
    /// * `cmd` - AddUserToGroupCommand containing user and group HRNs
    ///
    /// # Returns
    /// * Ok(()) if the user was successfully added to the group
    /// * Err(AddUserToGroupError) if there was an error
    pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), AddUserToGroupError> {
        // Parse and validate HRNs
        let user_hrn = Hrn::from_string(&cmd.user_hrn)
            .ok_or_else(|| AddUserToGroupError::InvalidUserHrn(cmd.user_hrn.clone()))?;

        let group_hrn = Hrn::from_string(&cmd.group_hrn)
            .ok_or_else(|| AddUserToGroupError::InvalidGroupHrn(cmd.group_hrn.clone()))?;

        // Find the user
        let user_dto = self
            .user_finder
            .find_user_by_hrn(&user_hrn)
            .await?
            .ok_or_else(|| AddUserToGroupError::UserNotFound(cmd.user_hrn.clone()))?;

        // Find the group
        let _group_dto = self
            .group_finder
            .find_group_by_hrn(&group_hrn)
            .await?
            .ok_or_else(|| AddUserToGroupError::GroupNotFound(cmd.group_hrn.clone()))?;

        // Add user to group by creating updated DTO
        let mut updated_group_hrns = user_dto.group_hrns.clone();
        if !updated_group_hrns.contains(&group_hrn.to_string()) {
            updated_group_hrns.push(group_hrn.to_string());
        }

        // Create updated user DTO for persistence
        let updated_user_dto = UserPersistenceDto {
            hrn: user_dto.hrn,
            name: user_dto.name,
            email: user_dto.email,
            group_hrns: updated_group_hrns,
            tags: user_dto.tags,
        };

        // Persist the updated user
        self.user_persister.save_user(&updated_user_dto).await?;

        Ok(())
    }
}
