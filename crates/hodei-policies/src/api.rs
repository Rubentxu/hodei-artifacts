//! Public API surface for the `hodei-policies` bounded context.
//!
//! This module defines the **explicit** public API that external consumers can use.
//! It follows the principle of **encapsulation** by exposing only the necessary interfaces:
//! - Use case ports (traits)
//! - DTOs (Commands, Queries, Results)
//! - Errors
//!
//! ## What is NOT exposed:
//! - Internal implementation details
//! - Use case concrete implementations (only through factories)
//! - Infrastructure adapters (wired in composition root)
//! - Mocks and test utilities
//!
//! ## Design Rationale:
//! By explicitly listing what is public, we:
//! 1. Prevent accidental coupling to internal implementation details
//! 2. Make refactoring safer (internal changes don't break consumers)
//! 3. Provide a clear contract for what this bounded context offers
//! 4. Follow the Interface Segregation Principle (ISP)

// ============================================================================
// FEATURE: build_schema
// ============================================================================
pub mod build_schema {
    // Direct exports for convenience
    pub use crate::features::build_schema::error::BuildSchemaError;
    pub use crate::features::build_schema::use_case::BuildSchemaUseCase;
    
    // Re-export as submodules for path compatibility (hodei_policies::build_schema::dto::*)
    pub mod dto {
        pub use crate::features::build_schema::dto::*;
    }
    pub mod ports {
        pub use crate::features::build_schema::ports::*;
    }
    pub mod error {
        pub use crate::features::build_schema::error::*;
    }
    pub mod factories {
        pub use crate::features::build_schema::factories::*;
    }
}

// ============================================================================
// FEATURE: evaluate_policies
// ============================================================================
pub mod evaluate_policies {
    pub use crate::features::evaluate_policies::error::EvaluatePoliciesError;
    pub use crate::features::evaluate_policies::use_case::EvaluatePoliciesUseCase;
    
    // Re-export dto, ports and factories as submodules
    pub mod dto {
        pub use crate::features::evaluate_policies::dto::*;
    }
    pub mod ports {
        pub use crate::features::evaluate_policies::ports::*;
    }
    pub mod factories {
        pub use crate::features::evaluate_policies::factories::*;
    }
}

// ============================================================================
// FEATURE: load_schema
// ============================================================================
pub mod load_schema {
    pub use crate::features::load_schema::error::LoadSchemaError;
    pub use crate::features::load_schema::use_case::LoadSchemaUseCase;
    
    // Re-export dto, ports and factories as submodules
    pub mod dto {
        pub use crate::features::load_schema::dto::*;
    }
    pub mod ports {
        pub use crate::features::load_schema::ports::*;
    }
    pub mod factories {
        pub use crate::features::load_schema::factories::*;
    }
}

// ============================================================================
// FEATURE: playground_evaluate
// ============================================================================
pub mod playground_evaluate {
    pub use crate::features::playground_evaluate::error::PlaygroundEvaluateError;
    pub use crate::features::playground_evaluate::use_case::PlaygroundEvaluateUseCase;
    
    // Re-export dto, ports, adapters and factories as submodules
    pub mod dto {
        pub use crate::features::playground_evaluate::dto::*;
    }
    pub mod ports {
        pub use crate::features::playground_evaluate::ports::*;
    }
    pub mod adapters {
        pub use crate::features::playground_evaluate::adapters::*;
    }
    pub mod factories {
        pub use crate::features::playground_evaluate::factories::*;
    }
}

// ============================================================================
// FEATURE: register_action_type
// ============================================================================
pub mod register_action_type {
    pub use crate::features::register_action_type::error::RegisterActionTypeError;
    pub use crate::features::register_action_type::use_case::RegisterActionTypeUseCase;
    
    // Re-export dto and ports as submodules for compatibility
    pub mod dto {
        pub use crate::features::register_action_type::dto::*;
    }
    pub mod ports {
        pub use crate::features::register_action_type::ports::*;
    }
}

// ============================================================================
// FEATURE: register_entity_type
// ============================================================================
pub mod register_entity_type {
    pub use crate::features::register_entity_type::error::RegisterEntityTypeError;
    pub use crate::features::register_entity_type::use_case::RegisterEntityTypeUseCase;
    
    // Re-export dto and ports as submodules for compatibility
    pub mod dto {
        pub use crate::features::register_entity_type::dto::*;
    }
    pub mod ports {
        pub use crate::features::register_entity_type::ports::*;
    }
}

// ============================================================================
// FEATURE: validate_policy
// ============================================================================
pub mod validate_policy {
    pub use crate::features::validate_policy::error::ValidatePolicyError;
    
    // Re-export dto, port and factories as submodules
    pub mod dto {
        pub use crate::features::validate_policy::dto::*;
    }
    pub mod port {
        pub use crate::features::validate_policy::port::*;
    }
    pub mod factories {
        pub use crate::features::validate_policy::factories::*;
    }
}
