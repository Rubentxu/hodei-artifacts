
// crates/repository/src/features/create_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, RepositoryId, UserId, Hrn};
use shared::lifecycle::Lifecycle;
use time::OffsetDateTime;
use tracing::{info, error, instrument, debug};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::{Repository, RepositoryConfig, RepositoryType};
use crate::domain::events::{RepositoryEvent, RepositoryCreated};
use super::dto::{CreateRepositoryCommand, CreateRepositoryResponse};
use super::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort, 
    StorageBackendExistsPort, EventPublisherPort
};

/// Caso de uso para crear un nuevo repositorio
pub struct CreateRepositoryUseCase {
    pub organization_exists_port: Arc<dyn OrganizationExistsPort>,
    pub repository_exists_port: Arc<dyn RepositoryExistsPort>,
    pub repository_creator_port: Arc<dyn RepositoryCreatorPort>,
    pub storage_backend_exists_port: Arc<dyn StorageBackendExistsPort>,
    pub event_publisher_port: Arc<dyn EventPublisherPort>,
}

impl CreateRepositoryUseCase {
    pub fn new(
        organization_exists_port: Arc<dyn OrganizationExistsPort>,
        repository_exists_port: Arc<dyn RepositoryExistsPort>,
        repository_creator_port: Arc<dyn RepositoryCreatorPort>,
        storage_backend_exists_port: Arc<dyn StorageBackendExistsPort>,
        event_publisher_port: Arc<dyn EventPublisherPort>,
    ) -> Self {
        Self {
            organization_exists_port,
            repository_exists_port,
            repository_creator_port,
            storage_backend_exists_port,
            event_publisher_port,
        }
    }

    fn validate_repository_name(&self, name: &str) -> RepositoryResult<()> {
        debug!("Validating repository name: {}", name);
        if name.is_empty() {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot be empty".to_string()));
        }
        if name.len() > 100 {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot exceed 100 characters".to_string()));
        }
        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(RepositoryError::InvalidRepositoryName("Repository name can only contain alphanumeric characters, hyphens, and underscores".to_string()));
        }
        if name.starts_with('-') || name.starts_with('_') || name.ends_with('-') || name.ends_with('_') {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot start or end with hyphens or underscores".to_string()));
        }
        debug!("Repository name '{}' is valid", name);
        Ok(())
    }

    fn validate_repository_config(&self, repo_type: &RepositoryType, config: &RepositoryConfig) -> RepositoryResult<()> {
        debug!("Validating repository config for type: {:?}", repo_type);
        match (repo_type, config) {
            (RepositoryType::Hosted, RepositoryConfig::Hosted(_)) => Ok(()),
            (RepositoryType::Proxy, RepositoryConfig::Proxy(proxy_config)) => {
                if proxy_config.remote_url.scheme() != "http" && proxy_config.remote_url.scheme() != "https" {
                    return Err(RepositoryError::InvalidConfiguration("Proxy repository remote URL must use HTTP or HTTPS protocol".to_string()));
                }
                Ok(())
            },
            (RepositoryType::Virtual, RepositoryConfig::Virtual(virtual_config)) => {
                if virtual_config.aggregated_repositories.len() < 2 {
                    return Err(RepositoryError::InvalidConfiguration("Virtual repository must aggregate at least 2 repositories".to_string()));
                }
                Ok(())
            },
            _ => Err(RepositoryError::RepositoryTypeMismatch),
        }
    }

    #[instrument(skip(self, command, organization_id, user_id))]
    pub async fn execute(
        &self,
        command: CreateRepositoryCommand,
        organization_id: OrganizationId,
        user_id: UserId,
    ) -> RepositoryResult<CreateRepositoryResponse> {
        info!("Creating repository '{}' for organization '{}'", command.name, organization_id);

        // 1. Validar que la organización existe
        if !self.organization_exists_port.organization_exists(&organization_id).await? {
            error!("Organization not found: {}", organization_id);
            return Err(RepositoryError::OrganizationNotFound(organization_id.to_string()));
        }

        // 2. Validar que el nombre del repositorio es válido
        self.validate_repository_name(&command.name)?;

        // 3. Verificar que no existe un repositorio con el mismo nombre en la organización
        if self.repository_exists_port.repository_exists(&organization_id, &command.name).await? {
            error!("Repository '{}' already exists in organization '{}'", command.name, organization_id);
            return Err(RepositoryError::RepositoryAlreadyExists(command.name));
        }

        // 4. Convertir la configuración del DTO al modelo de dominio
        let domain_config: RepositoryConfig = command.config.into();

        // 5. Validar la configuración según el tipo de repositorio
        self.validate_repository_config(&command.repo_type, &domain_config)?;

        // 6. Para repositorios Hosted, verificar que el backend de almacenamiento existe
        if let Some(storage_backend_hrn) = &command.storage_backend_hrn {
            if !self.storage_backend_exists_port.storage_backend_exists(storage_backend_hrn).await? {
                error!("Storage backend not found: {}", storage_backend_hrn);
                return Err(RepositoryError::StorageBackendNotFound(storage_backend_hrn.to_string()));
            }
        }

        // 7. Crear el HRN del repositorio
        let repository_id = RepositoryId::new(&organization_id.to_string(), &command.name)?;

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
            storage_backend_hrn: command.storage_backend_hrn.unwrap_or_else(|| Hrn::new(&format!("hrn:hodei:storage:::{}:default", organization_id)).unwrap()),
            lifecycle: Lifecycle::new(user_id.clone()),
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
        
        self.event_publisher_port.publish_repository_created(&event).await?;

        info!("Repository '{}' created successfully with HRN: {}", command.name, repository_id);

        // 11. Retornar la respuesta
        Ok(CreateRepositoryResponse {
            hrn: repository_id.to_string(),
            name: command.name,
            repo_type: command.repo_type,
            format: command.format,
            created_at: now,
        })
    }
}
