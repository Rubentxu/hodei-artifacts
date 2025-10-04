use crate::shared::domain::ServiceControlPolicy;
use crate::shared::application::ports::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use async_trait::async_trait;

/// Adapter that implements the ScpPersister trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpPersister for ScpRepositoryAdapter<SR> {
    /// Save an SCP using the repository
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        self.repository.save(&scp).await
    }
}
