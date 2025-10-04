use super::use_case::AddUserToGroupUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
/// Dependency Injection for add_user_to_group feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo)
}

pub fn make_use_case_with_events(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo).with_event_publisher(event_bus)
}
