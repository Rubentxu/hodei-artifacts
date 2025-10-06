use crate::features::create_account::adapter::CreateAccountSurrealUnitOfWorkFactoryAdapter;
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::internal::infrastructure::surreal::SurrealUnitOfWorkFactory;
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Create an instance of the CreateAccountUseCase with SurrealDB UoW
pub fn create_account_use_case(
    uow_factory: Arc<SurrealUnitOfWorkFactory>,
    partition: String,
    account_id: String,
) -> CreateAccountUseCase<CreateAccountSurrealUnitOfWorkFactoryAdapter> {
    let factory_adapter = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(uow_factory);
    CreateAccountUseCase::new(Arc::new(factory_adapter), partition, account_id)
}

/// Create an instance of the CreateAccountUseCase with event bus integration
pub fn create_account_use_case_with_events(
    uow_factory: Arc<SurrealUnitOfWorkFactory>,
    partition: String,
    account_id: String,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateAccountUseCase<CreateAccountSurrealUnitOfWorkFactoryAdapter> {
    let factory_adapter = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(uow_factory);
    CreateAccountUseCase::new(Arc::new(factory_adapter), partition, account_id)
        .with_event_publisher(event_bus)
}
