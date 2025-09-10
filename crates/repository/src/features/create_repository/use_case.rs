// crates/repository/src/features/create_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, RepositoryId, UserId};
use shared::lifecycle::Lifecycle;
use time::OffsetDateTime;
use tracing::{info, error, instrument};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::{Repository, RepositoryConfig};
use crate::domain::events::{RepositoryEvent, RepositoryCreated};
use super::dto::{CreateRepositoryCommand, CreateRepositoryResponse};
use super::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort, 
    StorageBackendExistsPort, EventPublisherPort, RepositoryNameValidatorPort,
    RepositoryConfigValidatorPort
};

/// Caso de uso para crear un nuevo repositorio
pub struct CreateRepositoryUseCase {
    organization_exists_port: Arc<dyn OrganizationExistsPort>,
    repository_exists_port: Arc<dyn RepositoryExistsPort>,
    repository_creator_port: Arc<dyn RepositoryCreatorPort>,
    storage_backend_exists_port: Arc<dyn StorageBackendExistsPort>,
    event_publisher_port: Arc<dyn EventPublisherPort>,
    name_validator_port: Arc<dyn RepositoryNameValidatorPort>,
    config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
}

impl CreateRepositoryUseCase {
    pub fn new(
        organization_exists_port: Arc<dyn OrganizationExistsPort>,
        repository_exists_port: Arc<dyn RepositoryExistsPort>,
        repository_creator_port: Arc<dyn RepositoryCreatorPort>,
        storage_backend_exists_port: Arc<dyn StorageBackendExistsPort>,
        event_publisher_port: Arc<dyn EventPublisherPort>,
        name_validator_port: Arc<dyn RepositoryNameValidatorPort>,
        config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
    ) -> Self {
        Self {
            organization_exists_port,
            repository_exists_port,
            repository_creator_port,
            storage_backend_exists_port,
            event_publisher_port,
            name_validator_port,
            config_validator_port,
        }
    }

    #[instrument(skip(self, command, organization_id, user_id))]
    pub async fn execute(
        &self,
        command: CreateRepositoryCommand,
        organization_id: OrganizationId,
        user_id: UserId,
    ) -> RepositoryResult<CreateRepositoryResponse> {
        info!("Creating repository '{}' for organization '{}'", command.name, organization_id.as_str());

        // 1. Validar que la organización existe
        if !self.organization_exists_port.organization_exists(&organization_id).await? {
            error!("Organization not found: {}", organization_id.as_str());
            return Err(RepositoryError::OrganizationNotFound(organization_id.as_str().to_string()));
        }

        // 2. Validar que el nombre del repositorio es válido
        self.name_validator_port.validate_repository_name(&command.name)?;

        // 3. Verificar que no existe un repositorio con el mismo nombre en la organización
        if self.repository_exists_port.repository_exists(&organization_id, &command.name).await? {
            error!("Repository '{}' already exists in organization '{}'", command.name, organization_id.as_str());
            return Err(RepositoryError::RepositoryAlreadyExists(command.name));
        }

        // 4. Convertir la configuración del DTO al modelo de dominio
        let domain_config = RepositoryConfig::try_from(command.config.clone())?;

        // 5. Validar la configuración según el tipo de repositorio
        self.config_validator_port.validate_repository_config(&command.repo_type, &domain_config)?;

        // 6. Para repositorios Hosted, verificar que el backend de almacenamiento existe
        if let Some(storage_backend_hrn) = &command.storage_backend_hrn {
            if !self.storage_backend_exists_port.storage_backend_exists(storage_backend_hrn).await? {
                error!("Storage backend not found: {}", storage_backend_hrn);
                return Err(RepositoryError::StorageBackendNotFound(storage_backend_hrn.clone()));
            }
        }

        // 7. Crear el HRN del repositorio
        let repository_id = RepositoryId::new(&organization_id, &command.name)
            .map_err(|e| RepositoryError::InvalidRepositoryName(format!("Failed to create repository HRN: {}", e)))?;

        // 8. Crear el modelo de dominio del repositorio
        let now = OffsetDateTime::now_utc();
        let repository = Repository {
            hrn: repository_id.clone(),
            organization_hrn: organization_id.clone(),
            name: command.name.clone(),
            region: "us-east-1".to_string(), // Default region, should be configurable
            repo_type: command.repo_type,
            format: command.format,
            config: domain_config,
            storage_backend_hrn: command.storage_backend_hrn.clone().unwrap_or_else(|| {
                format!("hrn:hodei:repository:us-east-1:{}:storage-backend/default", organization_id.as_str())
            }),
            lifecycle: Lifecycle {
                created_at: now,
                created_by: user_id.0.clone(),
                updated_at: now,
                updated_by: user_id.0.clone(),
            },
        };

        // 9. Guardar el repositorio
        self.repository_creator_port.create_repository(&repository).await?;

        // 10. Publicar evento de repositorio creado
        let event = RepositoryEvent::RepositoryCreated(RepositoryCreated {
            hrn: repository_id.clone(),
            name: command.name.clone(),
            repo_type: command.repo_type,
            format: command.format,
            organization_hrn: organization_id,
            at: now,
        });
        
        self.event_publisher_port.publish_repository_created(&repository).await?;

        info!("Repository '{}' created successfully with HRN: {}", command.name, repository_id.as_str());

        // 11. Retornar la respuesta
        Ok(CreateRepositoryResponse {
            hrn: repository_id.as_str().to_string(),
            name: command.name,
            repo_type: command.repo_type,
            format: command.format,
            created_at: now,
        })
    }
}