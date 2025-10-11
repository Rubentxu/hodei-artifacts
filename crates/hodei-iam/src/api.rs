//! Public API surface for the `hodei-iam` bounded context.
//!
//! This module defines the **explicit** public API that external consumers can use.
//! It follows the principle of **encapsulation** by exposing only the necessary interfaces:
//! - Use case ports (traits)
//! - DTOs (Commands, Queries, Views, Responses)
//! - Errors
//!
//! ## What is NOT exposed:
//! - Internal domain entities (`internal/domain`)
//! - Use case implementations (concrete structs)
//! - Infrastructure adapters (these are wired in the composition root)
//! - Mocks and test utilities
//!
//! ## Design Rationale:
//! By explicitly listing what is public, we:
//! 1. Prevent accidental coupling to internal implementation details
//! 2. Make refactoring safer (internal changes don't break consumers)
//! 3. Provide a clear contract for what this bounded context offers
//! 4. Follow the Interface Segregation Principle (ISP)

// ============================================================================
// FEATURE: create_user
// ============================================================================
pub mod create_user {
    pub use crate::features::create_user::dto::{CreateUserCommand, UserView};
    pub use crate::features::create_user::error::CreateUserError;
    pub use crate::features::create_user::ports::{CreateUserPort, CreateUserUseCasePort};
    pub use crate::features::create_user::use_case::CreateUserUseCase;
}

// ============================================================================
// FEATURE: create_group
// ============================================================================
pub mod create_group {
    pub use crate::features::create_group::dto::{CreateGroupCommand, GroupView};
    pub use crate::features::create_group::error::CreateGroupError;
    pub use crate::features::create_group::ports::{CreateGroupPort, CreateGroupUseCasePort};
    pub use crate::features::create_group::use_case::CreateGroupUseCase;
}

// ============================================================================
// FEATURE: add_user_to_group
// ============================================================================
pub mod add_user_to_group {
    pub use crate::features::add_user_to_group::dto::{
        AddUserToGroupCommand, GroupLookupDto, UserLookupDto, UserPersistenceDto,
    };
    pub use crate::features::add_user_to_group::error::AddUserToGroupError;
    pub use crate::features::add_user_to_group::ports::{
        AddUserToGroupUseCasePort, GroupFinder, UserFinder, UserGroupPersister,
    };
    pub use crate::features::add_user_to_group::use_case::AddUserToGroupUseCase;
}

// ============================================================================
// FEATURE: create_policy
// ============================================================================
pub mod create_policy {
    pub use crate::features::create_policy::dto::{CreatePolicyCommand, PolicyView};
    pub use crate::features::create_policy::error::CreatePolicyError;
    pub use crate::features::create_policy::ports::{
        CreatePolicyPort, CreatePolicyUseCasePort, PolicyValidationError, PolicyValidator,
        ValidationResult,
    };
    pub use crate::features::create_policy::use_case::CreatePolicyUseCase;
    pub use crate::features::create_policy::validator::CedarPolicyValidator;
    
    // Re-export factories for DI
    pub mod factories {
        pub use crate::features::create_policy::factories::*;
    }
}

// ============================================================================
// FEATURE: get_policy
// ============================================================================
pub mod get_policy {
    pub use crate::features::get_policy::dto::{GetPolicyQuery, PolicyView};
    pub use crate::features::get_policy::error::GetPolicyError;
    pub use crate::features::get_policy::ports::PolicyReader;
    pub use crate::features::get_policy::use_case::GetPolicyUseCase;
}

// ============================================================================
// FEATURE: list_policies
// ============================================================================
pub mod list_policies {
    pub use crate::features::list_policies::dto::{
        ListPoliciesQuery, ListPoliciesResponse, PageInfo, PolicySummary,
    };
    pub use crate::features::list_policies::error::ListPoliciesError;
    pub use crate::features::list_policies::ports::PolicyLister;
    pub use crate::features::list_policies::use_case::ListPoliciesUseCase;
}

// ============================================================================
// FEATURE: update_policy
// ============================================================================
pub mod update_policy {
    pub use crate::features::update_policy::dto::{PolicyView, UpdatePolicyCommand};
    pub use crate::features::update_policy::error::UpdatePolicyError;
    pub use crate::features::update_policy::ports::{
        PolicyValidationError, PolicyValidator, UpdatePolicyPort, ValidationResult,
    };
    pub use crate::features::update_policy::use_case::UpdatePolicyUseCase;
}

// ============================================================================
// FEATURE: delete_policy
// ============================================================================
pub mod delete_policy {
    pub use crate::features::delete_policy::dto::DeletePolicyCommand;
    pub use crate::features::delete_policy::error::DeletePolicyError;
    pub use crate::features::delete_policy::ports::DeletePolicyPort;
    pub use crate::features::delete_policy::use_case::DeletePolicyUseCase;
}

// ============================================================================
// FEATURE: register_iam_schema
// ============================================================================
pub mod register_iam_schema {
    // Direct exports for convenience
    pub use crate::features::register_iam_schema::error::RegisterIamSchemaError;
    pub use crate::features::register_iam_schema::use_case::RegisterIamSchemaUseCase;
    
    // Re-export as submodules for path compatibility
    pub mod dto {
        pub use crate::features::register_iam_schema::dto::*;
    }
    pub mod ports {
        pub use crate::features::register_iam_schema::ports::*;
    }
    pub mod factories {
        pub use crate::features::register_iam_schema::factories::*;
    }
}

// ============================================================================
// FEATURE: evaluate_iam_policies
// ============================================================================
pub mod evaluate_iam_policies {
    pub use crate::features::evaluate_iam_policies::error::EvaluateIamPoliciesError;
    pub use crate::features::evaluate_iam_policies::ports::{
        PolicyFinderError, PolicyFinderPort, PrincipalResolverPort,
    };
    pub use crate::features::evaluate_iam_policies::use_case::EvaluateIamPoliciesUseCase;
}

// ============================================================================
// FEATURE: get_effective_policies
// ============================================================================
pub mod get_effective_policies {
    pub use crate::features::get_effective_policies::dto::{
        EffectivePoliciesResponse, GetEffectivePoliciesQuery,
    };
    pub use crate::features::get_effective_policies::error::{
        GetEffectivePoliciesError, GetEffectivePoliciesResult,
    };
    pub use crate::features::get_effective_policies::ports::{
        GroupFinderPort, PolicyFinderPort, UserFinderPort,
    };
    pub use crate::features::get_effective_policies::use_case::GetEffectivePoliciesUseCase;
}

// ============================================================================
// INFRASTRUCTURE (Only for Composition Root / DI)
// ============================================================================
// Infrastructure adapters are exposed ONLY for dependency injection in the
// composition root. Application code should NOT depend on these directly.
pub mod infrastructure {
    pub use crate::infrastructure::hrn_generator::UuidHrnGenerator;
    pub use crate::infrastructure::surreal::{
        SurrealGroupAdapter, SurrealPolicyAdapter, SurrealUserAdapter,
    };
}
