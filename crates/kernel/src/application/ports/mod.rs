//! Application ports for the shared kernel
//!
//! This module contains the contract definitions (ports) that define
//! the interfaces between the application layer and infrastructure layer.
pub mod event_bus;
pub mod unit_of_work;
// Cross-context (shared kernel) ports for IAM and Organizations
pub mod iam {
    use async_trait::async_trait;
    use cedar_policy::PolicySet;
    use serde::{Deserialize, Serialize};

    /// Query DTO for obtaining effective IAM policies for a principal
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EffectivePoliciesQuery {
        pub principal_hrn: String,
    }

    /// Result DTO containing the resolved policy set and metadata
    #[derive(Debug, Clone)]
    pub struct EffectivePoliciesResult {
        pub policies: PolicySet,
        pub policy_count: usize,
    }

    /// Cross-context abstraction to obtain effective identity-based policies.
    ///
    /// Implemented by the IAM bounded context as an adapter around its
    /// internal use case; consumed by the authorizer (and others) without
    /// acoplarse a detalles internos de IAM.
    #[async_trait]
    pub trait EffectivePoliciesQueryPort: Send + Sync {
        async fn get_effective_policies(
            &self,
            query: EffectivePoliciesQuery,
        ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>>;
    }
}

pub mod organizations {
    use async_trait::async_trait;
    use cedar_policy::PolicySet;

    /// Query DTO for obtaining effective Service Control Policies (SCPs)
    #[derive(Debug, Clone)]
    pub struct GetEffectiveScpsQuery {
        pub resource_hrn: String,
    }

    /// Cross-context abstraction to obtain effective SCP constraints
    /// for a given resource (account / OU).
    #[async_trait]
    pub trait GetEffectiveScpsPort: Send + Sync {
        async fn get_effective_scps(
            &self,
            query: GetEffectiveScpsQuery,
        ) -> Result<PolicySet, Box<dyn std::error::Error + Send + Sync>>;
    }
}

// Re-export commonly used types
pub use event_bus::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};
pub use iam::{EffectivePoliciesQuery, EffectivePoliciesQueryPort, EffectivePoliciesResult};
pub use organizations::{GetEffectiveScpsPort, GetEffectiveScpsQuery};
pub use unit_of_work::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};
