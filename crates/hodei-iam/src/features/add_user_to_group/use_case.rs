/// Use case for adding a user to a group

use std::sync::Arc;
use crate::shared::application::ports::{UserRepository, GroupRepository};
use super::dto::AddUserToGroupCommand;
use policies::shared::domain::hrn::Hrn;

pub struct AddUserToGroupUseCase {
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
}

impl AddUserToGroupUseCase {
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        group_repo: Arc<dyn GroupRepository>,
    ) -> Self {
        Self { user_repo, group_repo }
    }

    pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), anyhow::Error> {
        // Parse HRNs
        let user_hrn = Hrn::from_string(&cmd.user_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid user HRN: {}", cmd.user_hrn))?;
        let group_hrn = Hrn::from_string(&cmd.group_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid group HRN: {}", cmd.group_hrn))?;

        // Validate that the group exists to maintain consistency
        if self.group_repo.find_by_hrn(&group_hrn).await?.is_none() {
            return Err(anyhow::anyhow!("Group not found: {}", cmd.group_hrn));
        }

        // Load the user
        let mut user = self.user_repo.find_by_hrn(&user_hrn).await?
            .ok_or_else(|| anyhow::anyhow!("User not found: {}", cmd.user_hrn))?;

        // Add user to group (domain logic handles idempotency)
        user.add_to_group(group_hrn);

        // Persist the updated user
        self.user_repo.save(&user).await?;

        Ok(())
    }
}
