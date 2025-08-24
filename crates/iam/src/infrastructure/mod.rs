//! Infrastructure layer for IAM bounded context
//!
//! This module contains adapters that implement the ports defined in application layer.
//! Following Hexagonal Architecture principles:
//! - Infrastructure depends on domain, never the reverse
//! - Concrete implementations of abstract ports
//! - External system integrations (HTTP, DB, messaging)

pub mod persistence;
pub mod messaging;
pub mod http;

// Re-export commonly used infrastructure components
pub use persistence::*;
pub use messaging::*;
pub use http::*;
