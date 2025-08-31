use std::sync::Arc;
use shared::RepositoryId;
use crate::application::ports::RepositoryStore;
use crate::domain::model::Repository;
use crate::error::RepositoryError;

/// Comando para obtener un repositorio por ID.
#[derive(Debug, Clone)]
pub struct GetRepositoryCommand {
    pub repository_id: RepositoryId,
}

/// Handler para el comando de obtención de repositorio.
pub struct GetRepositoryHandler<S>
where
    S: RepositoryStore,
{
    store: Arc<S>,
}

impl<S> GetRepositoryHandler<S>
where
    S: RepositoryStore,
{
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Ejecuta la lógica de negocio para obtener un repositorio.
    pub async fn handle(
        &self,
        cmd: GetRepositoryCommand,
    ) -> Result<Repository, RepositoryError> {
        let repo = self.store.get(&cmd.repository_id).await?;
        repo.ok_or(RepositoryError::NotFound)
    }
}
