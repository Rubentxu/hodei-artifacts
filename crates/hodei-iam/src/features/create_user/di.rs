use super::adapter::GenericCreateUserUnitOfWork;
use super::use_case::CreateUserUseCase;
use crate::shared::application::ports::UserRepository;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Dependency Injection for create_user feature with Unit of Work

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
) -> CreateUserUseCase<GenericCreateUserUnitOfWork> {
    let create_user_uow = Arc::new(GenericCreateUserUnitOfWork::new(user_repo));
    CreateUserUseCase::new(create_user_uow)
}

pub fn make_use_case_with_events(
    user_repo: Arc<dyn UserRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateUserUseCase<GenericCreateUserUnitOfWork> {
    let create_user_uow = Arc::new(GenericCreateUserUnitOfWork::new(user_repo));
    CreateUserUseCase::new(create_user_uow).with_event_publisher(event_bus)
}
