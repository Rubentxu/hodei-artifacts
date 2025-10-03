use super::use_case::AddUserToGroupUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
/// Dependency Injection for add_user_to_group feature

use std::sync::Arc;

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo)
}

