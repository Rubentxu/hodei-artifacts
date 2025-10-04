use super::adapter::GenericCreateGroupUnitOfWork;
use super::use_case::CreateGroupUseCase;
use crate::shared::application::ports::GroupRepository;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Dependency Injection for create_group feature with Unit of Work
pub fn make_use_case(
    group_repo: Arc<dyn GroupRepository>,
) -> CreateGroupUseCase<GenericCreateGroupUnitOfWork> {
    let create_group_uow = Arc::new(GenericCreateGroupUnitOfWork::new(group_repo));
    CreateGroupUseCase::new(create_group_uow)
}

pub fn make_use_case_with_events(
    group_repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateGroupUseCase<GenericCreateGroupUnitOfWork> {
    let create_group_uow = Arc::new(GenericCreateGroupUnitOfWork::new(group_repo));
    CreateGroupUseCase::new(create_group_uow).with_event_publisher(event_bus)
}
