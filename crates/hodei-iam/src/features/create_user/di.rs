use super::use_case::CreateUserUseCase;
use crate::shared::application::ports::UserRepository;
/// Dependency Injection for create_user feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn UserRepository>) -> CreateUserUseCase {
    CreateUserUseCase::new(repo)
}

pub fn make_use_case_with_events(
    repo: Arc<dyn UserRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateUserUseCase {
    CreateUserUseCase::new(repo).with_event_publisher(event_bus)
}
