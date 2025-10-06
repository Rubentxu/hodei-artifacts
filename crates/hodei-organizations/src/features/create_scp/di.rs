use crate::features::create_scp::adapter::SurrealScpPersister;
use crate::features::create_scp::use_case::{
    CreateScpUseCase, DeleteScpUseCase, GetScpUseCase, ListScpsUseCase, UpdateScpUseCase,
};
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

fn build_persister(db: &Arc<Surreal<Any>>) -> SurrealScpPersister {
    // Clone the connection so each persister owns its copy of the handle.
    SurrealScpPersister::new(db.as_ref().clone())
}

/// Assemble the create SCP use case with a SurrealDB-backed persister.
pub fn make_create_scp_use_case(
    db: Arc<Surreal<Any>>,
) -> CreateScpUseCase<SurrealScpPersister> {
    CreateScpUseCase::new(build_persister(&db))
}

/// Assemble the delete SCP use case with a SurrealDB-backed persister.
pub fn make_delete_scp_use_case(
    db: Arc<Surreal<Any>>,
) -> DeleteScpUseCase<SurrealScpPersister> {
    DeleteScpUseCase::new(build_persister(&db))
}

/// Assemble the update SCP use case with a SurrealDB-backed persister.
pub fn make_update_scp_use_case(
    db: Arc<Surreal<Any>>,
) -> UpdateScpUseCase<SurrealScpPersister> {
    UpdateScpUseCase::new(build_persister(&db))
}

/// Assemble the get SCP use case with a SurrealDB-backed persister.
pub fn make_get_scp_use_case(
    db: Arc<Surreal<Any>>,
) -> GetScpUseCase<SurrealScpPersister> {
    GetScpUseCase::new(build_persister(&db))
}

/// Assemble the list SCPs use case with a SurrealDB-backed persister.
pub fn make_list_scps_use_case(
    db: Arc<Surreal<Any>>,
) -> ListScpsUseCase<SurrealScpPersister> {
    ListScpsUseCase::new(build_persister(&db))
}
