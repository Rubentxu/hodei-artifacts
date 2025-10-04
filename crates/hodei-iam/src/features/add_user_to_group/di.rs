use super::adapter::GenericAddUserToGroupUnitOfWork;
use super::use_case::AddUserToGroupUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Dependency Injection for add_user_to_group feature with Unit of Work
pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase<GenericAddUserToGroupUnitOfWork> {
    let add_user_uow = Arc::new(GenericAddUserToGroupUnitOfWork::new(user_repo, group_repo));
    AddUserToGroupUseCase::new(add_user_uow)
}

pub fn make_use_case_with_events(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> AddUserToGroupUseCase<GenericAddUserToGroupUnitOfWork> {
    let add_user_uow = Arc::new(GenericAddUserToGroupUnitOfWork::new(user_repo, group_repo));
    AddUserToGroupUseCase::new(add_user_uow).with_event_publisher(event_bus)
}
