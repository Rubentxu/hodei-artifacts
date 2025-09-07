use std::sync::Arc;
use shared::{RepositoryId, UserId, IsoTimestamp};
use crate::application::ports::{RepositoryStore, EventBus};
use crate::domain::event::RepositoryDeletedEvent;
use crate::error::RepositoryError;

/// Comando para eliminar un repositorio por ID.
#[derive(Debug, Clone)]
pub struct DeleteRepositoryCommand {
    pub repository_id: RepositoryId,
    pub deleted_by: UserId,
    pub occurred_at: IsoTimestamp,
}

/// Handler para el comando de eliminación de repositorio.
pub struct DeleteRepositoryHandler<S, E>
where
    S: RepositoryStore,
    E: EventBus,
{
    store: Arc<S>,
    event_bus: Arc<E>,
}

impl<S, E> DeleteRepositoryHandler<S, E>
where
    S: RepositoryStore,
    E: EventBus,
{
    pub fn new(store: Arc<S>, event_bus: Arc<E>) -> Self {
        Self { store, event_bus }
    }

    /// Ejecuta la lógica de negocio para eliminar un repositorio.
    pub async fn handle(
        &self,
        cmd: DeleteRepositoryCommand,
    ) -> Result<(), RepositoryError> {
        // Check if repository exists
        let repo = self.store.get(&cmd.repository_id).await?;
        if repo.is_none() {
            return Err(RepositoryError::NotFound);
        }

        // Delete from store
        self.store.delete(&cmd.repository_id).await?;

        // Publish event
        let event_payload = RepositoryDeletedEvent {
            repository_id: cmd.repository_id,
            deleted_by: cmd.deleted_by,
            occurred_at: cmd.occurred_at,
        };
        let envelope = shared::domain::event::DomainEventEnvelope::new_root(event_payload, None);
        self.event_bus.publish(&envelope).await?;

        Ok(())
    }
}
