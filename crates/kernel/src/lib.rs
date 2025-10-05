// crates/shared/src/lib.rs

// pub mod events;  // Temporalmente desactivado - depende de Hrn
// pub mod lifecycle;  // Temporalmente desactivado - depende de Hrn
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-export application types for ergonomic use
pub use application::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};

// Re-export event bus types for ergonomic use
pub use application::ports::{
    DomainEvent,
    // Cross-context IAM ports
    EffectivePoliciesQuery,
    EffectivePoliciesQueryPort,
    EffectivePoliciesResult,
    EventBus,
    EventEnvelope,
    EventHandler,
    EventPublisher,
    GetEffectiveScpsPort,
    // Cross-context Organizations ports
    GetEffectiveScpsQuery,
    Subscription,
};

// Re-export infrastructure implementations
pub use infrastructure::InMemoryEventBus;

// Re-export shared domain (kernel) symbols
pub use domain::{
    ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, Hrn, PolicyStorage,
    PolicyStorageError, Principal, Resource,
};
