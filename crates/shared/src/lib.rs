// crates/shared/src/lib.rs


// pub mod events;  // Temporalmente desactivado - depende de Hrn
// pub mod lifecycle;  // Temporalmente desactivado - depende de Hrn
pub mod application;
pub mod infrastructure;



// Re-export application types for ergonomic use
pub use application::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};

// Re-export event bus types for ergonomic use
pub use application::ports::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};

// Re-export infrastructure implementations
pub use infrastructure::InMemoryEventBus;
