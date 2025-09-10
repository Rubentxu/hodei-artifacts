// crates/repository/src/features/update_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{RepositoryId, UserId};
use tracing::{info, error, instrument, warn};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::dto::{UpdateRepositoryCommand, UpdateRepositoryResponse};
use super::ports::{
    RepositoryUpdaterPort, RepositoryUpdateAuthorizationPort, RepositoryConfigValidatorPort,
    RepositoryUpdateEventPublisherPort
};

/// Caso de uso para actualizar un repositorio
pub struct UpdateRepositoryUseCase {
    repository_updater_port: Arc<dyn RepositoryUpdaterPort>,
    authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
    config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
    event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
}

impl UpdateRepositoryUseCase {
    pub fn new(
        repository_updater_port: Arc<dyn RepositoryUpdaterPort>,
        authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
        config_validator_port: Arc<dyn RepositoryConfigValidatorPort>,
        event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
    ) -> Self {
        Self {
            repository_updater_port,
            authorization_port,
            config_validator_port,
            event_publisher_port,
        }
    }

    #[instrument(skip(self, command, user_id))]
    pub async fn execute(
        &self,
        command: UpdateRepositoryCommand,
        user_id: UserId,
    ) -> RepositoryResult<UpdateRepositoryResponse> {
        info!("Updating repository with HRN: {}", command.repository_hrn);

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
        if !self.authorization_port.can_update_repository(&user_id, &repository_id).await? {
            error!("User {} is not authorized to update repository {}", user_id.as_str(), repository_id.as_str());
            return Err(RepositoryError::Unauthorized(
                format!("You don't have permission to update repository '{}'", repository_id.as_str())
            ));
        }

        // 3. Obtener el repositorio actual para actualización
        let mut repository = self.repository_updater_port.get_repository_for_update(&repository_id).await?
            .ok_or_else(|| {
                error!("Repository not found for update: {}", repository_id.as_str());
                RepositoryError::RepositoryNotFound(repository_id.as_str().to_string())
            })?;

        info!("Found repository for update: {}", repository.name);

        // 4. Validar que el tipo de repositorio no cambie
        if let Some(new_config) = &command.config {
            let new_repo_type = Self::extract_repository_type_from_config(new_config);
            self.config_validator_port.validate_type_consistency(repository.repo_type, new_repo_type).await?;
        }

        // 5. Aplicar actualizaciones
        let mut changes = Vec::new();
        
        // Actualizar descripción
        if let Some(description) = command.description {
            // TODO: Agregar campo description al modelo de dominio
            changes.push(format!("description updated"));
            warn!("Description field not implemented in domain model yet");
        }

        // Actualizar configuración
        if let Some(new_config) = command.config {
            let current_config = repository.config.clone();
            let updated_config = Self::apply_config_update(&current_config, new_config)?;
            
            // Validar la configuración actualizada
            self.config_validator_port.validate_config_update(&current_config, &updated_config).await?;
            
            repository.config = updated_config;
            changes.push("configuration updated".to_string());
        }

        // Actualizar backend de almacenamiento
        if let Some(storage_backend_hrn) = command.storage_backend_hrn {
            repository.storage_backend_hrn = storage_backend_hrn;
            changes.push("storage backend updated".to_string());
        }

        // Actualizar visibilidad
        if let Some(is_public) = command.is_public {
            // TODO: Agregar campo is_public al modelo de dominio
            changes.push(format!("visibility updated to {}", if is_public { "public" } else { "private" }));
            warn!("is_public field not implemented in domain model yet");
        }

        // Actualizar metadatos
        if let Some(metadata) = command.metadata {
            // TODO: Agregar campo metadata al modelo de dominio
            changes.push("metadata updated".to_string());
            warn!("Metadata field not implemented in domain model yet");
        }

        // Actualizar información de ciclo de vida
        repository.lifecycle.updated_at = time::OffsetDateTime::now_utc();
        repository.lifecycle.updated_by = user_id.0.clone();

        // 6. Persistir los cambios
        self.repository_updater_port.update_repository(&repository).await?;
        info!("Repository updated successfully: {}", repository_id.as_str());

        // 7. Publicar evento de actualización
        self.event_publisher_port.publish_repository_updated(&repository_id, &user_id, changes).await?;

        // 8. Construir la respuesta
        let response = UpdateRepositoryResponse::from(repository);
        
        info!("Successfully updated repository: {}", repository_id.as_str());

        Ok(response)
    }

    fn extract_repository_type_from_config(config: &super::dto::RepositoryConfigUpdateDto) -> crate::domain::repository::RepositoryType {
        match config {
            super::dto::RepositoryConfigUpdateDto::Hosted(_) => crate::domain::repository::RepositoryType::Hosted,
            super::dto::RepositoryConfigUpdateDto::Proxy(_) => crate::domain::repository::RepositoryType::Proxy,
            super::dto::RepositoryConfigUpdateDto::Virtual(_) => crate::domain::repository::RepositoryType::Virtual,
        }
    }

    fn apply_config_update(
        current_config: &crate::domain::repository::RepositoryConfig,
        update_dto: super::dto::RepositoryConfigUpdateDto,
    ) -> RepositoryResult<crate::domain::repository::RepositoryConfig> {
        use crate::domain::repository::{RepositoryConfig, HostedConfig, ProxyConfig, VirtualConfig};
        use super::dto::{RepositoryConfigUpdateDto, HostedConfigUpdateDto, ProxyConfigUpdateDto, VirtualConfigUpdateDto};

        match (current_config, update_dto) {
            (RepositoryConfig::Hosted(current), RepositoryConfigUpdateDto::Hosted(update)) => {
                let new_config = HostedConfig {
                    deployment_policy: update.deployment_policy
                        .map(|dto| Self::convert_deployment_policy_dto(dto))
                        .unwrap_or(current.deployment_policy),
                };
                Ok(RepositoryConfig::Hosted(new_config))
            },
            (RepositoryConfig::Proxy(current), RepositoryConfigUpdateDto::Proxy(update)) => {
                let new_config = ProxyConfig {
                    remote_url: update.remote_url
                        .map(|url| url::Url::parse(&url))
                        .transpose()
                        .map_err(|e| RepositoryError::InvalidConfiguration(format!("Invalid URL: {}", e)))?
                        .unwrap_or(current.remote_url.clone()),
                    cache_settings: if let Some(cache_update) = update.cache_settings {
                        CacheSettings {
                            metadata_ttl_seconds: cache_update.metadata_ttl_seconds.unwrap_or(current.cache_settings.metadata_ttl_seconds),
                            artifact_ttl_seconds: cache_update.artifact_ttl_seconds.unwrap_or(current.cache_settings.artifact_ttl_seconds),
                        }
                    } else {
                        current.cache_settings.clone()
                    },
                    remote_authentication: if let Some(auth_update) = update.remote_authentication {
                        Some(ProxyAuth {
                            username: auth_update.username.unwrap_or_else(|| {
                                current.remote_authentication.as_ref().map(|a| a.username.clone()).unwrap_or_default()
                            }),
                            password_secret_hrn: if let Some(secret_hrn) = auth_update.password_secret_hrn {
                                shared::hrn::Hrn::new(&secret_hrn).map_err(|e| {
                                    RepositoryError::InvalidConfiguration(format!("Invalid secret HRN: {}", e))
                                })?
                            } else {
                                current.remote_authentication.as_ref()
                                    .map(|a| a.password_secret_hrn.clone())
                                    .unwrap_or_else(|| shared::hrn::Hrn::new("hrn:hodei:security::secret/default").unwrap())
                            },
                        })
                    } else {
                        current.remote_authentication.clone()
                    },
                };
                Ok(RepositoryConfig::Proxy(new_config))
            },
            (RepositoryConfig::Virtual(current), RepositoryConfigUpdateDto::Virtual(update)) => {
                let new_config = VirtualConfig {
                    aggregated_repositories: update.aggregated_repositories
                        .map(|repos| {
                            repos.into_iter()
                                .map(|repo_hrn| {
                                    shared::hrn::RepositoryId::new(
                                        &shared::hrn::OrganizationId::new("system").unwrap(), // TODO: Extraer de HRN
                                        &repo_hrn
                                    ).map_err(|e| RepositoryError::InvalidConfiguration(format!("Invalid repository HRN: {}", e)))
                                })
                                .collect::<RepositoryResult<Vec<_>>>()
                        })
                        .transpose()?
                        .unwrap_or(current.aggregated_repositories.clone()),
                    resolution_order: update.resolution_order
                        .map(|dto| Self::convert_resolution_order_dto(dto))
                        .unwrap_or(current.resolution_order),
                };
                Ok(RepositoryConfig::Virtual(new_config))
            },
            _ => Err(RepositoryError::RepositoryTypeMismatch {
                expected: format!("{:?}", current_config),
                actual: format!("{:?}", update_dto),
            }),
        }
    }

    fn convert_deployment_policy_dto(dto: super::dto::DeploymentPolicyUpdateDto) -> crate::domain::repository::DeploymentPolicy {
        use super::dto::DeploymentPolicyUpdateDto;
        match dto {
            DeploymentPolicyUpdateDto::AllowSnapshots => crate::domain::repository::DeploymentPolicy::AllowSnapshots,
            DeploymentPolicyUpdateDto::BlockSnapshots => crate::domain::repository::DeploymentPolicy::BlockSnapshots,
            DeploymentPolicyUpdateDto::AllowRedeploy => crate::domain::repository::DeploymentPolicy::AllowRedeploy,
            DeploymentPolicyUpdateDto::BlockRedeploy => crate::domain::repository::DeploymentPolicy::BlockRedeploy,
        }
    }

    fn convert_resolution_order_dto(dto: super::dto::ResolutionOrderUpdateDto) -> crate::domain::repository::ResolutionOrder {
        use super::dto::ResolutionOrderUpdateDto;
        match dto {
            ResolutionOrderUpdateDto::FirstFound => crate::domain::repository::ResolutionOrder::FirstFound,
        }
    }
}