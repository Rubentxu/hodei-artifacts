// crates/repository/src/features/get_repository/use_case.rs

use std::sync::Arc;
use shared::hrn::{RepositoryId, UserId};
use tracing::{info, error, instrument};

use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::dto::{GetRepositoryQuery, GetRepositoryResponse};
use super::ports::{RepositoryReaderPort, RepositoryAuthorizationPort, RepositoryStatsPort};

/// Caso de uso para obtener un repositorio
pub struct GetRepositoryUseCase {
    pub repository_reader_port: Arc<dyn RepositoryReaderPort>,
    pub authorization_port: Arc<dyn RepositoryAuthorizationPort>,
    pub stats_port: Arc<dyn RepositoryStatsPort>,
}

impl GetRepositoryUseCase {
    pub fn new(
        repository_reader_port: Arc<dyn RepositoryReaderPort>,
        authorization_port: Arc<dyn RepositoryAuthorizationPort>,
        stats_port: Arc<dyn RepositoryStatsPort>,
    ) -> Self {
        Self {
            repository_reader_port,
            authorization_port,
            stats_port,
        }
    }

    #[instrument(skip(self, query, user_id))]
    pub async fn execute(
        &self,
        query: GetRepositoryQuery,
        user_id: UserId,
    ) -> RepositoryResult<GetRepositoryResponse> {
        info!("Getting repository with HRN: {}", query.repository_hrn);

        // 1. Parsear y validar el HRN del repositorio
        let repository_id = RepositoryId::new(
            &shared::hrn::OrganizationId::new("system").unwrap(), // TODO: Extraer de la query o contexto
            &query.repository_hrn
        ).map_err(|e| {
            error!("Invalid repository HRN: {}", e);
            RepositoryError::InvalidRepositoryName(format!("Invalid repository HRN: {}", e))
        })?;

        info!("Parsed repository ID: {}", repository_id.as_str());

        // 2. Verificar autorización
        if !self.authorization_port.can_read_repository(&user_id, &repository_id).await? {
            error!("User {} is not authorized to read repository {}", user_id.as_str(), repository_id.as_str());
            return Err(RepositoryError::Unauthorized(
                format!("You don't have permission to read repository '{}'", repository_id.as_str())
            ));
        }

        // 3. Obtener el repositorio
        let repository = self.repository_reader_port.get_repository(&repository_id).await?
            .ok_or_else(|| {
                error!("Repository not found: {}", repository_id.as_str());
                RepositoryError::RepositoryNotFound(repository_id.as_str().to_string())
            })?;

        info!("Found repository: {}", repository.name);

        // 4. Obtener estadísticas del repositorio
        let stats = self.repository_reader_port.get_repository_stats(&repository_id).await?;
        
        info!("Repository stats - artifacts: {}, size: {} bytes, downloads: {}", 
               stats.artifact_count, stats.total_size_bytes, stats.total_downloads);

        // 5. Construir la respuesta
        let mut response = GetRepositoryResponse::from(repository);
        
        // Actualizar estadísticas en la respuesta
        response.stats = GetRepositoryResponse::stats_from(stats);

        info!("Successfully retrieved repository: {}", repository_id.as_str());

        Ok(response)
    }
}

// Helper para convertir estadísticas
impl GetRepositoryResponse {
    fn stats_from(stats: super::ports::RepositoryStats) -> crate::features::get_repository::dto::RepositoryStatsResponse {
        crate::features::get_repository::dto::RepositoryStatsResponse {
            artifact_count: stats.artifact_count,
            total_size_bytes: stats.total_size_bytes,
            last_artifact_uploaded_at: stats.last_artifact_uploaded_at,
            total_downloads: stats.total_downloads,
        }
    }
}