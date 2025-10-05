//! hodei-organizations: Organization structure and Service Control Policies
//!
//! This crate manages organizational units, accounts, and SCPs.
//! External access is ONLY through features (use cases).
//!
//! # Architecture
//! - Internal: domain entities, repositories, adapters
//! - External: ONLY features/use cases with Commands/Queries/DTOs

pub mod features;
mod shared;

// ✅ Re-export domain events for external event subscribers
pub mod events {
    pub use crate::shared::domain::events::{AccountCreated, ScpAttached};
}

// ❌ NO exportar entidades de dominio - son INTERNAS
// Solo se accede a este crate a través de sus casos de uso (features)

// ✅ Re-export features/casos de uso para acceso externo
pub use features::{
    attach_scp::use_case::AttachScpUseCase,
    create_account::use_case::CreateAccountUseCase,
    create_ou::use_case::CreateOuUseCase,
    get_effective_scps::{dto::EffectiveScpsResponse, use_case::GetEffectiveScpsUseCase},
};

// -----------------------------------------------------------------------------
// Adaptador Cross-Context: Expone el caso de uso interno get_effective_scps
// mediante el puerto transversal definido en el kernel compartido
// (`shared::GetEffectiveScpsPort`), desacoplando consumidores externos de
// detalles internos del bounded context Organizations.
// -----------------------------------------------------------------------------
use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use ::kernel::application::ports::GetEffectiveScpsPort; // import only port; fully qualify query type to avoid name clash
use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::sync::Arc;

/// Adaptador genérico que implementa el puerto compartido a partir del
/// caso de uso interno parametrizado.
pub struct GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: ScpRepositoryPort + Send + Sync,
    OrgRepo: OuRepositoryPort + AccountRepositoryPort + Send + Sync,
{
    inner: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>,
}

impl<ScpRepo, OrgRepo> GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: ScpRepositoryPort + Send + Sync,
    OrgRepo: OuRepositoryPort + AccountRepositoryPort + Send + Sync,
{
    pub fn new(inner: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<ScpRepo, OrgRepo> GetEffectiveScpsPort for GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: ScpRepositoryPort + Send + Sync,
    OrgRepo: OuRepositoryPort + AccountRepositoryPort + Send + Sync,
{
    async fn get_effective_scps(
        &self,
        query: ::kernel::application::ports::GetEffectiveScpsQuery,
    ) -> Result<PolicySet, Box<dyn std::error::Error + Send + Sync>> {
        // Convertir el DTO transversal al DTO interno del caso de uso
        let internal_query = features::get_effective_scps::dto::GetEffectiveScpsQuery {
            resource_hrn: query.resource_hrn,
        };

        // Ejecutar caso de uso interno (retorna EffectiveScpsResponse)
        let response = self
            .inner
            .execute(internal_query)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        // Mapear al tipo del puerto transversal (PolicySet)
        Ok(response.policies)
    }
}

/// Alias ergonómico para inyección dinámica
pub type DynGetEffectiveScpsPort = Arc<dyn GetEffectiveScpsPort>;
