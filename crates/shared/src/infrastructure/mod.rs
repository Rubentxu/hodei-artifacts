//! Infrastructure layer for shared services and adapters

pub mod audit;
pub mod in_memory_event_bus;
pub mod surrealdb_adapter;

// Re-export commonly used infrastructure types
pub use audit::{AuditEventHandler, AuditLog, AuditLogStore, AuditStats};
pub use in_memory_event_bus::InMemoryEventBus;
