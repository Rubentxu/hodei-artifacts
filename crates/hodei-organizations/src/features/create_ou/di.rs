use crate::features::create_ou::adapter::CreateOuSurrealUnitOfWorkFactoryAdapter;
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::internal::infrastructure::surreal::SurrealUnitOfWorkFactory;
use std::sync::Arc;

/// Create an instance of the CreateOuUseCase with SurrealDB UoW
pub fn create_ou_use_case<C>(
    uow_factory: Arc<SurrealUnitOfWorkFactory<C>>,
) -> CreateOuUseCase<CreateOuSurrealUnitOfWorkFactoryAdapter<C>>
where
    C: surrealdb::Connection,
{
    let factory_adapter = CreateOuSurrealUnitOfWorkFactoryAdapter::new(uow_factory);
    CreateOuUseCase::new(Arc::new(factory_adapter))
}
