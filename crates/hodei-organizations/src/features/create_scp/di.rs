use crate::features::create_scp::adapter::CreateScpSurrealUnitOfWorkFactoryAdapter;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::internal::infrastructure::surreal::SurrealUnitOfWorkFactory;
use std::sync::Arc;

/// Create an instance of the CreateScpUseCase with SurrealDB UoW
pub fn create_scp_use_case(
    uow_factory: Arc<SurrealUnitOfWorkFactory>,
) -> CreateScpUseCase<CreateScpSurrealUnitOfWorkFactoryAdapter> {
    let factory_adapter = CreateScpSurrealUnitOfWorkFactoryAdapter::new(uow_factory);
    CreateScpUseCase::new(Arc::new(factory_adapter))
}
