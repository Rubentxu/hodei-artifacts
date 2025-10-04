use super::use_case::CreateGroupUseCase;
use crate::shared::application::ports::GroupRepository;
/// Dependency Injection for create_group feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn GroupRepository>) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo)
}

pub fn make_use_case_with_events(
    repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo).with_event_publisher(event_bus)
}
