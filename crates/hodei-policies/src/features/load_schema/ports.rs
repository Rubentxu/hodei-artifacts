//! Ports (trait definitions) for the load_schema feature
//!
//! This module re-exports the SchemaStoragePort from build_schema
//! since both features share the same storage abstraction.
//!
//! Following the DRY principle and ISP (Interface Segregation Principle),
//! we reuse the existing storage port rather than defining a duplicate.

// Re-export the SchemaStoragePort from build_schema
pub use crate::features::build_schema::ports::SchemaStoragePort;
