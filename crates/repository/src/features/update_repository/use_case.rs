
// crates/repository/src/features/update_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{RepositoryId, UserId, Hrn};
use tracing::{info, error, instrument, warn};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::{Repository, RepositoryConfig, RepositoryType, CacheSettings, ProxyAuth, DeploymentPolicy, ResolutionOrder};
use super::dto::{UpdateRepositoryCommand, UpdateRepositoryResponse};
use super::ports::{
    RepositoryUpdaterPort, RepositoryUpdateAuthorizationPort, RepositoryUpdateEventPublisherPort
};

/// Caso de uso para actualizar un repositorio
pub struct UpdateRepositoryUseCase {
    pub repository_updater_port: Arc<dyn RepositoryUpdaterPort>,
    pub authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
    pub event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
}

impl UpdateRepositoryUseCase {
    pub fn new(
        repository_updater_port: Arc<dyn RepositoryUpdaterPort>,
        authorization_port: Arc<dyn RepositoryUpdateAuthorizationPort>,
        event_publisher_port: Arc<dyn RepositoryUpdateEventPublisherPort>,
    ) -> Self {
        Self {
            repository_updater_port,
            authorization_port,
            event_publisher_port,
        }
    }
    
    async fn validate_config_update(&self, current_config: &RepositoryConfig, new_config: &RepositoryConfig) -> RepositoryResult<()> {
        // For now, we only check for type consistency. More complex validation can be added here.
        self.validate_type_consistency(current_config.get_type(), new_config.get_type()).await
    }

    async fn validate_type_consistency(&self, current_type: RepositoryType, new_type: RepositoryType) -> RepositoryResult<()> {
        if current_type != new_type {
            Err(RepositoryError::RepositoryTypeMismatch)
        } else {
            Ok(())
        }
    }

    #[instrument(skip(self, command, user_id))]
    pub async fn execute(
        &self,
        command: UpdateRepositoryCommand,
        user_id: UserId,
    ) -> RepositoryResult<UpdateRepositoryResponse> {
        info!("Updating repository with HRN: {}", command.repository_hrn);

        let repository_id: RepositoryId = command.repository_hrn.parse()?;

        info!("Parsed repository ID: {}", repository_id);

        if !self.authorization_port.can_update_repository(&user_id, &repository_id).await? {
            error!("User {} is not authorized to update repository {}", user_id, repository_id);
            return Err(RepositoryError::Unauthorized(
                format!("You don't have permission to update repository '{}'", repository_id)
            ));
        }

        let mut repository = self.repository_updater_port.get_repository_for_update(&repository_id).await?
            .ok_or_else(|| {
                error!("Repository not found for update: {}", repository_id);
                RepositoryError::RepositoryNotFound(repository_id.to_string())
            })?;

        info!("Found repository for update: {}", repository.name);

        let mut changes = Vec::new();

        if let Some(new_config_dto) = command.config {
            let new_config = Self::apply_config_update(&repository.config, new_config_dto)?;
            self.validate_config_update(&repository.config, &new_config).await?;
            repository.config = new_config;
            changes.push("configuration updated".to_string());
        }

        if let Some(storage_backend_hrn) = command.storage_backend_hrn {
            repository.storage_backend_hrn = storage_backend_hrn;
            changes.push("storage backend updated".to_string());
        }

        repository.lifecycle.updated_at = time::OffsetDateTime::now_utc();
        repository.lifecycle.updated_by = user_id.clone();

        self.repository_updater_port.update_repository(&repository).await?;
        info!("Repository updated successfully: {}", repository_id);

        self.event_publisher_port.publish_repository_updated(&repository_id, &user_id, changes).await?;

        let response = UpdateRepositoryResponse::from(repository);
        
        info!("Successfully updated repository: {}", repository_id);

        Ok(response)
    }

    fn apply_config_update(
        current_config: &RepositoryConfig,
        update_dto: super::dto::RepositoryConfigUpdateDto,
    ) -> RepositoryResult<RepositoryConfig> {
        use crate::domain::repository::{HostedConfig, ProxyConfig, VirtualConfig};
        use super::dto::{HostedConfigUpdateDto, ProxyConfigUpdateDto, VirtualConfigUpdateDto};

        match (current_config, update_dto) {
            (RepositoryConfig::Hosted(current), super::dto::RepositoryConfigUpdateDto::Hosted(update)) => {
                let new_config = HostedConfig {
                    deployment_policy: update.deployment_policy.map(Self::convert_deployment_policy_dto).unwrap_or(current.deployment_policy),
                };
                Ok(RepositoryConfig::Hosted(new_config))
            },
            (RepositoryConfig::Proxy(current), super::dto::RepositoryConfigUpdateDto::Proxy(update)) => {
                let new_config = ProxyConfig {
                    remote_url: update.remote_url.map(|url| url.parse()).transpose()?.unwrap_or_else(|| current.remote_url.clone()),
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
                            username: auth_update.username.unwrap_or_else(|| current.remote_authentication.as_ref().map(|a| a.username.clone()).unwrap_or_default()),
                            password_secret_hrn: auth_update.password_secret_hrn.map(|hrn| hrn.parse()).transpose()?.unwrap_or_else(|| current.remote_authentication.as_ref().map(|a| a.password_secret_hrn.clone()).unwrap_or_else(|| Hrn::new("hrn:hodei:security::secret/default").unwrap())),
                        })
                    } else {
                        current.remote_authentication.clone()
                    },
                };
                Ok(RepositoryConfig::Proxy(new_config))
            },
            (RepositoryConfig::Virtual(current), super::dto::RepositoryConfigUpdateDto::Virtual(update)) => {
                let new_config = VirtualConfig {
                    aggregated_repositories: update.aggregated_repositories.map(|repos| repos.into_iter().map(|hrn| hrn.parse()).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_else(|| current.aggregated_repositories.clone()),
                    resolution_order: update.resolution_order.map(Self::convert_resolution_order_dto).unwrap_or(current.resolution_order),
                };
                Ok(RepositoryConfig::Virtual(new_config))
            },
            _ => Err(RepositoryError::RepositoryTypeMismatch),
        }
    }

    fn convert_deployment_policy_dto(dto: super::dto::DeploymentPolicyUpdateDto) -> DeploymentPolicy {
        match dto {
            super::dto::DeploymentPolicyUpdateDto::AllowSnapshots => DeploymentPolicy::AllowSnapshots,
            super::dto::DeploymentPolicyUpdateDto::BlockSnapshots => DeploymentPolicy::BlockSnapshots,
            super::dto::DeploymentPolicyUpdateDto::AllowRedeploy => DeploymentPolicy::AllowRedeploy,
            super::dto::DeploymentPolicyUpdateDto::BlockRedeploy => DeploymentPolicy::BlockRedeploy,
        }
    }

    fn convert_resolution_order_dto(dto: super::dto::ResolutionOrderUpdateDto) -> ResolutionOrder {
        match dto {
            super::dto::ResolutionOrderUpdateDto::FirstFound => ResolutionOrder::FirstFound,
        }
    }
}
