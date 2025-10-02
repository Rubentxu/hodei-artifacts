/// Dependency Injection for create_group feature

use std::sync::Arc;
use crate::shared::application::ports::GroupRepository;
use super::use_case::CreateGroupUseCase;

pub fn make_use_case(repo: Arc<dyn GroupRepository>) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo)
}

