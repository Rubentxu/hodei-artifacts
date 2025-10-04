use crate::features::create_ou::adapter::CreateOuSurrealUnitOfWorkFactoryAdapter;
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::shared::infrastructure::surreal::SurrealUnitOfWorkFactory;
use std::sync::Arc;

/// Create an instance of the CreateOuUseCase with SurrealDB UoW
pub fn create_ou_use_case(
    uow_factory: Arc<SurrealUnitOfWorkFactory>,
) -> CreateOuUseCase<CreateOuSurrealUnitOfWorkFactoryAdapter> {
    let factory_adapter = CreateOuSurrealUnitOfWorkFactoryAdapter::new(uow_factory);
    CreateOuUseCase::new(Arc::new(factory_adapter))
}
