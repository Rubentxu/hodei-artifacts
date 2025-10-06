// crates/shared/src/lib.rs

// pub mod events;  // Temporalmente desactivado - depende de Hrn
// pub mod lifecycle;  // Temporalmente desactivado - depende de Hrn
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-export application types for ergonomic use
pub use application::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};

// Re-export application ports for ergonomic use
pub use application::ports::{
    // Authentication and authorization
    AuthContextError,
    AuthContextProvider,
    AuthorizationError,
    // Event bus
    DomainEvent,
    // Cross-context IAM ports
    EffectivePoliciesQuery,
    EffectivePoliciesQueryPort,
    EffectivePoliciesResult,
    EvaluationDecision,
    EvaluationRequest,
    EventBus,
    EventEnvelope,
    EventHandler,
    EventPublisher,
    // Cross-context Organizations ports
    GetEffectiveScpsPort,
    GetEffectiveScpsQuery,
    IamPolicyEvaluator,
    ScpEvaluator,
    SessionMetadata,
    Subscription,
};

// Re-export infrastructure implementations
pub use infrastructure::InMemoryEventBus;

// Re-export shared domain (kernel) symbols
pub use domain::{
    ActionTrait, AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Hrn,
    PolicyStorage, PolicyStorageError, Principal, Resource, ResourceTypeName, ServiceName,
};
