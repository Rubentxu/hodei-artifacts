use crate::features::create_account::adapter::AccountRepositoryAdapter;
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::shared::application::ports::account_repository::AccountRepository;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Create an instance of the CreateAccountUseCase with the provided repository
#[allow(dead_code)]
pub(crate) fn create_account_use_case<AR: AccountRepository + Send + Sync>(
    account_repository: AR,
    partition: String,
    account_id: String,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(Arc::new(adapter), partition, account_id)
}

/// Create an instance of the CreateAccountUseCase with event bus integration
#[allow(dead_code)]
pub(crate) fn create_account_use_case_with_events<AR: AccountRepository + Send + Sync>(
    account_repository: AR,
    partition: String,
    account_id: String,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(Arc::new(adapter), partition, account_id)
        .with_event_publisher(event_bus)
}
