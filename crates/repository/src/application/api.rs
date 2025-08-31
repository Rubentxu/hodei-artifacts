use std::sync::Arc;

use crate::application::ports::{RepositoryStore, EventBus};
use crate::error::RepositoryError;
use crate::domain::model::Repository;
use crate::features::create_repository::{CreateRepositoryCommand, CreateRepositoryHandler, CreateRepositoryResponse};
use crate::features::get_repository::{GetRepositoryCommand, GetRepositoryHandler};
use crate::features::delete_repository::{DeleteRepositoryCommand, DeleteRepositoryHandler};
use iam::application::ports::Authorization;

pub struct RepositoryApi<S, E, A>
where
    S: RepositoryStore,
    E: EventBus,
    A: Authorization,
{
    create_handler: CreateRepositoryHandler<S, E>,
    get_handler: GetRepositoryHandler<S>,
    delete_handler: DeleteRepositoryHandler<S, E>,
    authorization: Arc<A>,
}

impl<S, E, A> RepositoryApi<S, E, A>
where
    S: RepositoryStore,
    E: EventBus,
    A: Authorization,
{
    pub fn new(store: Arc<S>, event_bus: Arc<E>, authorization: Arc<A>) -> Self {
        Self {
            create_handler: CreateRepositoryHandler::new(store.clone(), event_bus.clone()),
            get_handler: GetRepositoryHandler::new(store.clone()),
            delete_handler: DeleteRepositoryHandler::new(store.clone(), event_bus.clone()),
            authorization,
        }
    }

    pub async fn create_repository(&self, cmd: CreateRepositoryCommand) -> Result<CreateRepositoryResponse, RepositoryError> {
        // TODO: Add authorization check here if needed for API level
        self.create_handler.handle(cmd).await
    }

    pub async fn get_repository(&self, cmd: GetRepositoryCommand) -> Result<Repository, RepositoryError> {
        // TODO: Add authorization check here if needed for API level
        self.get_handler.handle(cmd).await
    }

    pub async fn delete_repository(&self, cmd: DeleteRepositoryCommand) -> Result<(), RepositoryError> {
        // TODO: Add authorization check here if needed for API level
        self.delete_handler.handle(cmd).await
    }
}
