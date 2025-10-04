//! Application ports for the shared kernel
//!
//! This module contains the contract definitions (ports) that define
//! the interfaces between the application layer and infrastructure layer.
pub mod event_bus;
pub mod unit_of_work;

// Re-export commonly used types
pub use event_bus::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};
pub use unit_of_work::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};
