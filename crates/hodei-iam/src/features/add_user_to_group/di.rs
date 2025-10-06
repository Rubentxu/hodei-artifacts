use super::adapter::GenericAddUserToGroupUnitOfWork;
use super::use_case::AddUserToGroupUseCase;
use crate::internal::application::ports::{GroupRepository, UserRepository};
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// Dependency Injection for add_user_to_group feature with Unit of Work
///
/// Creates a use case with real SurrealDB transaction support.
///
/// # Arguments
///
/// * `user_repo` - User repository implementation
/// * `group_repo` - Group repository implementation  
/// * `db` - SurrealDB connection for transaction management
pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    db: Surreal<Any>,
) -> AddUserToGroupUseCase<GenericAddUserToGroupUnitOfWork> {
    let add_user_uow = Arc::new(GenericAddUserToGroupUnitOfWork::new(user_repo, group_repo, db));
    AddUserToGroupUseCase::new(add_user_uow)
}

/// Create use case with event bus support
///
/// # Arguments
///
/// * `user_repo` - User repository implementation
/// * `group_repo` - Group repository implementation
/// * `db` - SurrealDB connection for transaction management
/// * `event_bus` - Event bus for publishing domain events
pub fn make_use_case_with_events(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    db: Surreal<Any>,
    event_bus: Arc<InMemoryEventBus>,
) -> AddUserToGroupUseCase<GenericAddUserToGroupUnitOfWork> {
    let add_user_uow = Arc::new(GenericAddUserToGroupUnitOfWork::new(user_repo, group_repo, db));
    AddUserToGroupUseCase::new(add_user_uow).with_event_publisher(event_bus)
}

/// Create use case for testing with in-memory repositories (no real DB needed)
///
/// This is used by integration tests that use in-memory repositories
/// and don't need real SurrealDB transactions. It creates a mock UnitOfWork
/// that uses the provided repositories directly.
///
/// # Arguments
///
/// * `user_repo` - User repository (typically InMemoryUserRepository)
/// * `group_repo` - Group repository (typically InMemoryGroupRepository)
///
/// # Note
///
/// This function is exposed for integration tests. The provided repositories
/// should be the ones that already contain the test data.
pub fn make_test_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase<crate::features::add_user_to_group::mocks::MockAddUserToGroupUnitOfWork> {
    use crate::features::add_user_to_group::mocks::MockAddUserToGroupUnitOfWork;
    
    // Create a mock UoW with the provided repositories
    let mock_uow = Arc::new(MockAddUserToGroupUnitOfWork::new(user_repo, group_repo));
    
    AddUserToGroupUseCase::new(mock_uow)
}
