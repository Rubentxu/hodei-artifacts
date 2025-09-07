//! Infrastructure layer for Supply Chain bounded context
//!
//! Contains adapters that implement ports for supply chain management
//! Following Hexagonal Architecture and VSA principles

pub mod persistence;
pub mod messaging;
pub mod http;

// Re-export infrastructure components
pub use persistence::*;
pub use messaging::*;
pub use http::*;
