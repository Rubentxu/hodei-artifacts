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
//! ```no_run
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
pub mod shared;

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

use async_trait::async_trait;
use std::sync::Arc;

/// Abstraction (puerto) para consultar políticas efectivas de un principal.
///
/// Este trait elimina la necesidad de que consumidores externos conozcan los parámetros genéricos
/// del caso de uso interno. Se implementa sobre el caso de uso genérico real y se expone
/// como objeto dinámico (`Arc<dyn EffectivePoliciesQueryService>`).
#[async_trait]
pub trait EffectivePoliciesQueryService: Send + Sync {
    async fn get_effective_policies(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> Result<
        EffectivePoliciesResponse,
        features::get_effective_policies_for_principal::GetEffectivePoliciesError,
    >;
}

/// Implementación del trait para el caso de uso genérico real.
#[async_trait]
impl<UF, GF, PF> EffectivePoliciesQueryService
    for features::get_effective_policies_for_principal::GetEffectivePoliciesForPrincipalUseCase<
        UF,
        GF,
        PF,
    >
where
    UF: features::get_effective_policies_for_principal::UserFinderPort + Send + Sync,
    GF: features::get_effective_policies_for_principal::GroupFinderPort + Send + Sync,
    PF: features::get_effective_policies_for_principal::PolicyFinderPort + Send + Sync,
{
    async fn get_effective_policies(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> Result<
        EffectivePoliciesResponse,
        features::get_effective_policies_for_principal::GetEffectivePoliciesError,
    > {
        self.execute(query).await
    }
}

/// Tipo de conveniencia para inyección de dependencias.
pub type DynEffectivePoliciesQueryService = Arc<dyn EffectivePoliciesQueryService>;

// ✅ Configurador para policies engine (necesario para setup inicial)
pub use shared::application::configure_default_iam_entities;
