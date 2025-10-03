use super::use_case::CreateGroupUseCase;
use crate::shared::application::ports::GroupRepository;
/// Dependency Injection for create_group feature

use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn GroupRepository>) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo)
}

