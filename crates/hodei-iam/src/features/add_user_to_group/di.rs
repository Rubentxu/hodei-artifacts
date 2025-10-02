/// Dependency Injection for add_user_to_group feature

use std::sync::Arc;
use crate::shared::application::ports::{UserRepository, GroupRepository};
use super::use_case::AddUserToGroupUseCase;

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo)
}

