use super::dto::{CreateGroupCommand, GroupView};
use crate::shared::{
    application::ports::GroupRepository,
    domain::Group,
};
use policies::shared::domain::hrn::Hrn;
/// Use case for creating a new group

use std::sync::Arc;

pub struct CreateGroupUseCase {
    repo: Arc<dyn GroupRepository>,
}

impl CreateGroupUseCase {
    pub fn new(repo: Arc<dyn GroupRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let group_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            group_id,
        );

        // Create the group domain entity
        let mut group = Group::new(hrn, cmd.group_name.clone());
        group.tags = cmd.tags.clone();

        // Persist the group
        self.repo.save(&group).await?;

        // Return the view
        Ok(GroupView {
            hrn: group.hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}
