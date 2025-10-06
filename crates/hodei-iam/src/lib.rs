//! # hodei-iam
//!
//! IAM (Identity and Access Management) Bounded Context for Hodei Artifacts.
//!
//! This crate provides IAM functionality following **Vertical Slice Architecture (VSA)**
//! with **Clean Architecture** principles.
//!
//! ## Architecture Principles
//!
//! - **Encapsulation**: Internal domain entities, repositories, and infrastructure
//!   are NOT exposed publicly. Only use case APIs are public.
//! - **Vertical Slice Architecture**: Each feature is self-contained with its own
//!   ports, adapters, DTOs, and use case.
//! - **Interface Segregation**: Each feature defines minimal, specific ports.
//!
//! ## Public API
//!
//! This crate exposes **only use cases (features)** through its public API.
//!
//! ### Available Features
//!
//! - **User Management**
//!   - `CreateUserUseCase`: Create a new IAM user
//!   - `AddUserToGroupUseCase`: Add a user to a group
//!
//! - **Group Management**
//!   - `CreateGroupUseCase`: Create a new IAM group
//!
//! - **Policy Management**
//!   - `CreatePolicyUseCase`: Create IAM policies (identity-based) (new segregated)
//!   - `GetEffectivePoliciesForPrincipalUseCase`: Query effective policies for a principal
//!
//! - **Authorization**
//!   - `EvaluateIamPoliciesUseCase`: Evaluate IAM policies for authorization decisions
//!
//! ## Usage Example
//!
//! ```ignore
//! use hodei_iam::{CreateUserUseCase, CreateUserCommand};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Use cases are injected with their dependencies through DI
//! let use_case = CreateUserUseCase::new(/* dependencies */);
//!
//! let command = CreateUserCommand {
//!     user_hrn: "hrn:hodei:iam::account123:user/alice".to_string(),
//!     name: "Alice".to_string(),
//!     email: Some("alice@example.com".to_string()),
//! };
//!
//! let result = use_case.execute(command).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Internal Structure (NOT PUBLIC)
//!
//! The `internal` module contains:
//! - Domain entities (User, Group, Policy)
//! - Repository ports
//! - Infrastructure adapters
//!
//! These are implementation details and should NOT be accessed directly.
//! All interactions must go through the public use case APIs.
use cedar_policy::PolicySet;
use std::str::FromStr;
// ============================================================================
// INTERNAL MODULE - NOT PUBLIC
// ============================================================================
// Contains domain entities, repositories, and infrastructure.
// This is an implementation detail and must not be exposed.
mod internal;
// ============================================================================
// PUBLIC MODULES
// ============================================================================
///// Public features/use cases module
pub mod features;
// ============================================================================
// PUBLIC RE-EXPORTS - USE CASES AND DTOs ONLY
// ============================================================================
// --- User Management ---
pub use features::add_user_to_group::{AddUserToGroupCommand, AddUserToGroupUseCase};
pub use features::create_user::{CreateUserCommand, CreateUserUseCase};
// --- Group Management ---
pub use features::create_group::{CreateGroupCommand, CreateGroupUseCase};
// --- Policy Management ---
// TODO: REFACTOR (Phase 2) - create_policy feature is temporarily disabled
// This feature is monolithic (contains full CRUD) and will be split into separate features:
// - create_policy, delete_policy, update_policy, get_policy, list_policies
// Each will follow ISP with segregated ports
// pub use features::create_policy::{
//     CreatePolicyCommand, CreatePolicyError, CreatePolicyUseCase, DeletePolicyCommand,
//     DeletePolicyUseCase, GetPolicyQuery, GetPolicyUseCase, ListPoliciesQuery, ListPoliciesUseCase,
//     PolicyView, UpdatePolicyCommand, UpdatePolicyUseCase,
// };

// New segregated policy features (Phase 2)
pub use features::create_policy_new::{
    CreatePolicyCommand, CreatePolicyError, CreatePolicyUseCase, PolicyView,
};

pub use features::get_policy::{
    GetPolicyUseCase,
    dto::{GetPolicyQuery, PolicyView as GetPolicyView},
    error::GetPolicyError,
};

pub use features::list_policies::{
    ListPoliciesUseCase,
    dto::{ListPoliciesQuery, ListPoliciesResponse, PageInfo, PolicySummary},
    error::ListPoliciesError,
};

pub use features::update_policy::{
    UpdatePolicyUseCase,
    dto::{UpdatePolicyCommand, PolicyView as UpdatePolicyView},
    error::UpdatePolicyError,
};

pub use features::delete_policy::{
    DeletePolicyUseCase,
    dto::DeletePolicyCommand,
    error::DeletePolicyError,
};

pub use features::get_effective_policies_for_principal::{
    EffectivePoliciesResponse, GetEffectivePoliciesForPrincipalUseCase, GetEffectivePoliciesQuery,
    make_use_case as make_get_effective_policies_use_case,
};
// --- Authorization ---
pub use features::evaluate_iam_policies::EvaluateIamPoliciesUseCase;
// ============================================================================
// DOMAIN EVENTS - PUBLIC FOR EVENT SUBSCRIBERS
// ============================================================================
/// Domain events emitted by IAM features.
/// These are public to allow external event subscribers (e.g., audit logs, notifications).
pub mod events {
    pub use crate::internal::domain::events::{GroupCreated, UserAddedToGroup, UserCreated};
}
// ============================================================================
// ADAPTER FOR KERNEL PORTS
// ============================================================================
use ::kernel::application::ports::{
    EffectivePoliciesQuery, EffectivePoliciesQueryPort, EffectivePoliciesResult,
};
use async_trait::async_trait;
use std::sync::Arc;
/// Adapter that exposes the internal "get_effective_policies_for_principal" use case
/// through the cross-cutting port defined in the shared kernel (`EffectivePoliciesQueryPort`).
///
/// This decouples consumers (e.g., authorizer) from internal details of the IAM bounded context,
/// adhering to DIP and avoiding unnecessary dependencies.
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
        // Translate cross-cutting DTO to internal use case DTO
        let internal_query =
            features::get_effective_policies_for_principal::GetEffectivePoliciesQuery {
                principal_hrn: query.principal_hrn,
            };
        let resp = self.inner.execute(internal_query).await?;
        // Convert Vec<String> policies to PolicySet
        let policy_set = if resp.policies.is_empty() {
            PolicySet::new()
        } else {
            // Concatenate all policy strings and parse as PolicySet
            let combined_policies = resp.policies.join("\n");
            PolicySet::from_str(&combined_policies)
                .map_err(|e| format!("Failed to parse policies: {}", e))?
        };
        Ok(EffectivePoliciesResult {
            policies: policy_set,
            policy_count: resp.policy_count,
        })
    }
}
/// Ergonomic alias for dynamic injection of the cross-cutting port
pub type DynEffectivePoliciesQueryPort = Arc<dyn EffectivePoliciesQueryPort>;
// ============================================================================
// POLICY ENGINE CONFIGURATOR - INTERNAL USE ONLY
// ============================================================================
// This is needed for initial setup of the policies engine with default IAM entities.
// It's a bridge to the legacy configuration system and should be refactored.
pub use internal::application::configure_default_iam_entities;
// ============================================================================
// TEMPORARY EXPORTS FOR DI CONFIGURATION - TO BE REMOVED
// ============================================================================
// TODO: These exports break encapsulation and should be removed once DI is refactored.
// The DI container should be configured at the application layer (main.rs or api crate)
// without needing direct access to infrastructure implementations.
#[doc(hidden)]
pub mod __internal_di_only {
    //! ⚠️ WARNING: This module is for DI configuration ONLY.
    //!
    //! These exports break encapsulation and will be removed in a future refactor.
    //! DO NOT use these in application code. Use the public use case APIs instead.
    pub use crate::internal::application::ports::{GroupRepository, UserRepository};
    pub use crate::internal::infrastructure::persistence::{
        InMemoryGroupRepository, InMemoryUserRepository,
    };
}
// Re-export for backward compatibility (to be removed)
#[deprecated(
    since = "0.1.0",
    note = "Direct infrastructure access violates encapsulation. Use __internal_di_only for DI config only."
)]
pub mod infrastructure {
    pub use super::__internal_di_only::{InMemoryGroupRepository, InMemoryUserRepository};
}
#[deprecated(
    since = "0.1.0",
    note = "Direct port access violates encapsulation. Use __internal_di_only for DI config only."
)]
pub mod ports {
    pub use super::__internal_di_only::{GroupRepository, UserRepository};
}
