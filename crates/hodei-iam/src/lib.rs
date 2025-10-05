//! hodei-iam: Default IAM entities for the policies engine
//!
//! This crate provides a standard set of Identity and Access Management entities
//! that can be used with the policies engine. It follows the same Vertical Slice
//! Architecture (VSA) with hexagonal architecture as the policies crate.
//!
//! # Structure
//! - `shared/domain`: Core IAM entities (User, Group, ServiceAccount, Namespace) and actions
//! - `shared/application`: Ports (repository traits) and DI configurator
//! - `shared/infrastructure`: Infrastructure adapters (in-memory repositories for testing)
//! - `features`: IAM-specific features/use cases (create_user, create_group, add_user_to_group)
//!
//! # Example
//! ```ignore
//! use hodei_iam::shared::application::configure_default_iam_entities;
//! use policies::shared::application::di_helpers;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Build an engine with default IAM entities
//! let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
//! # Ok(())
//! # }
//! ```
pub mod features;
// TODO: REFACTOR - shared should be private, but tests in tests/ directory need it
// Tests should either:
// 1. Be moved to src/shared/domain/*_test.rs for unit tests
// 2. Use only public features API for integration tests
mod shared;

// ✅ Re-export domain events for external event subscribers
pub mod events {
    pub use crate::shared::domain::events::{GroupCreated, UserAddedToGroup, UserCreated};
}

// ✅ Re-export infrastructure repositories for DI configuration
pub mod infrastructure {
    pub use crate::shared::infrastructure::persistence::{
        InMemoryGroupRepository, InMemoryUserRepository,
    };
}

// ✅ Re-export application ports for external DI configuration
pub mod ports {
    pub use crate::shared::application::ports::{GroupRepository, UserRepository};
}

// ❌ NO exportar entidades de dominio - son INTERNAS
// Solo se accede a este crate a través de sus casos de uso (features)

// ✅ Re-export features/casos de uso para acceso externo
pub use features::{
    add_user_to_group::AddUserToGroupUseCase,
    create_group::CreateGroupUseCase,
    create_user::CreateUserUseCase,
    get_effective_policies_for_principal::{
        EffectivePoliciesResponse, GetEffectivePoliciesQuery,
        make_use_case as make_get_effective_policies_use_case,
    },
};

use ::kernel::application::ports::{
    EffectivePoliciesQuery, EffectivePoliciesQueryPort, EffectivePoliciesResult,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Adaptador que expone el caso de uso interno de "get_effective_policies_for_principal"
/// mediante el puerto transversal definido en el kernel compartido (`EffectivePoliciesQueryPort`).
///
/// Esto desacopla a consumidores (p.ej. authorizer) de los detalles internos del
/// bounded context IAM, cumpliendo DIP y evitando dependencias innecesarias.
pub struct EffectivePoliciesAdapter<U> {
    inner: U,
}

impl<U> EffectivePoliciesAdapter<U> {
    pub fn new(inner: U) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<UF, GF, PF> EffectivePoliciesQueryPort
    for EffectivePoliciesAdapter<
        features::get_effective_policies_for_principal::GetEffectivePoliciesForPrincipalUseCase<
            UF,
            GF,
            PF,
        >,
    >
where
    UF: features::get_effective_policies_for_principal::UserFinderPort + Send + Sync,
    GF: features::get_effective_policies_for_principal::GroupFinderPort + Send + Sync,
    PF: features::get_effective_policies_for_principal::PolicyFinderPort + Send + Sync,
{
    async fn get_effective_policies(
        &self,
        query: EffectivePoliciesQuery,
    ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>> {
        // Traducir el DTO transversal al DTO interno del caso de uso
        let internal_query =
            features::get_effective_policies_for_principal::GetEffectivePoliciesQuery {
                principal_hrn: query.principal_hrn,
            };

        let resp = self.inner.execute(internal_query).await?;

        Ok(EffectivePoliciesResult {
            policies: resp.policies,
            policy_count: resp.policy_count,
        })
    }
}

/// Alias ergonómico para inyección dinámica del puerto transversal
pub type DynEffectivePoliciesQueryPort = Arc<dyn EffectivePoliciesQueryPort>;

// ✅ Configurador para policies engine (necesario para setup inicial)
pub use shared::application::configure_default_iam_entities;
