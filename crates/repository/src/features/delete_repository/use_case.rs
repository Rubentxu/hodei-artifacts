// crates/repository/src/features/delete_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{RepositoryId, UserId};
use tracing::{info, error, instrument, warn};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::dto::{DeleteRepositoryCommand, DeleteRepositoryResponse};
use super::ports::{
    RepositoryDeleterPort, RepositoryDeleteAuthorizationPort, ArtifactDeleterPort,
    RepositoryDeleteEventPublisherPort
};

/// Caso de uso para eliminar un repositorio
pub struct DeleteRepositoryUseCase {
    repository_deleter_port: Arc<dyn RepositoryDeleterPort>,
    authorization_port: Arc<dyn RepositoryDeleteAuthorizationPort>,
    artifact_deleter_port: Arc<dyn ArtifactDeleterPort>,
    event_publisher_port: Arc<dyn RepositoryDeleteEventPublisherPort>,
}

impl DeleteRepositoryUseCase {
    pub fn new(
        repository_deleter_port: Arc<dyn RepositoryDeleterPort>,
        authorization_port: Arc<dyn RepositoryDeleteAuthorizationPort>,
        artifact_deleter_port: Arc<dyn ArtifactDeleterPort>,
        event_publisher_port: Arc<dyn RepositoryDeleteEventPublisherPort>,
    ) -> Self {
        Self {
            repository_deleter_port,
            authorization_port,
            artifact_deleter_port,
            event_publisher_port,
        }
    }

    #[instrument(skip(self, command, user_id))]
    pub async fn execute(
        &self,
        command: DeleteRepositoryCommand,
        user_id: UserId,
    ) -> RepositoryResult<DeleteRepositoryResponse> {
        info!("Deleting repository with HRN: {}", command.repository_hrn);

        // 1. Parsear y validar el HRN del repositorio
        let repository_id = RepositoryId::new(
            &shared::hrn::OrganizationId::new("system").unwrap(), // TODO: Extraer de la query o contexto
            &command.repository_hrn
        ).map_err(|e| {
            error!("Invalid repository HRN: {}", e);
            RepositoryError::InvalidRepositoryName(format!("Invalid repository HRN: {}", e))
        })?;

        info!("Parsed repository ID: {}", repository_id.as_str());

        // 2. Verificar autorización
        if !self.authorization_port.can_delete_repository(&user_id, &repository_id).await? {
            error!("User {} is not authorized to delete repository {}", user_id.as_str(), repository_id.as_str());
            return Err(RepositoryError::Unauthorized(
                format!("You don't have permission to delete repository '{}'", repository_id.as_str())
            ));
        }

        // 3. Obtener el repositorio antes de eliminarlo
        let repository = self.repository_deleter_port.get_repository_for_deletion(&repository_id).await?
            .ok_or_else(|| {
                error!("Repository not found for deletion: {}", repository_id.as_str());
                RepositoryError::RepositoryNotFound(repository_id.as_str().to_string())
            })?;

        info!("Found repository for deletion: {}", repository.name);

        // 4. Verificar si el repositorio está vacío (a menos que se fuerce la eliminación)
        if !command.force {
            let is_empty = self.repository_deleter_port.is_repository_empty(&repository_id).await?;
            if !is_empty {
                let artifact_count = self.artifact_deleter_port.count_repository_artifacts(&repository_id).await?;
                warn!("Repository {} is not empty ({} artifacts), deletion blocked without force flag", 
                      repository.name, artifact_count);
                return Err(RepositoryError::RepositoryNotEmpty(
                    format!("Repository '{}' contains {} artifacts. Use force=true to delete anyway.", 
                           repository.name, artifact_count)
                ));
            }
        }

        // 5. Obtener estadísticas antes de eliminar
        let artifact_count = self.artifact_deleter_port.count_repository_artifacts(&repository_id).await?;
        let total_size_bytes = 0; // TODO: Calcular tamaño total desde base de datos o storage

        info!("Repository {} has {} artifacts, total size: {} bytes", 
              repository.name, artifact_count, total_size_bytes);

        // 6. Eliminar artefactos asociados (si hay alguno)
        if artifact_count > 0 {
            info!("Deleting {} artifacts from repository {}", artifact_count, repository.name);
            let deleted_count = self.artifact_deleter_port.delete_repository_artifacts(&repository_id).await?;
            info!("Successfully deleted {} artifacts", deleted_count);
            
            if deleted_count != artifact_count {
                warn!("Expected to delete {} artifacts but only deleted {}", artifact_count, deleted_count);
            }
        }

        // 7. Eliminar el repositorio
        self.repository_deleter_port.delete_repository(&repository_id).await?;
        info!("Repository deleted successfully: {}", repository_id.as_str());

        // 8. Publicar evento de eliminación
        self.event_publisher_port.publish_repository_deleted(&repository_id, &user_id, artifact_count, total_size_bytes).await?;

        // 9. Construir la respuesta
        let mut response = DeleteRepositoryResponse::from(repository);
        response.deleted_by = user_id.as_str().to_string();
        response.deleted_at = time::OffsetDateTime::now_utc();
        response.final_stats.artifact_count = artifact_count;
        response.final_stats.total_size_bytes = total_size_bytes;
        
        info!("Successfully deleted repository: {}", repository_id.as_str());

        Ok(response)
    }
}