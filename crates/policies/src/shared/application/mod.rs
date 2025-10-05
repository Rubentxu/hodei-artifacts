//! Application layer for the policies crate
//!
//! This layer contains the authorization engine and its supporting components.
//!
//! ## Modules
//!
//! - `engine` - Authorization engine with agnostic API (ALWAYS AVAILABLE)
//! - `di_helpers` - Dependency injection helpers (behind `legacy_infra` flag)
//!
//! ## Architecture
//!
//! The new `engine` module is ALWAYS available and provides a clean, agnostic API.
//! The legacy infrastructure (gated behind `legacy_infra` feature) is being phased out.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use policies::shared::application::engine::{AuthorizationEngine, EngineRequest};
//!
//! let engine = AuthorizationEngine::new();
//! engine.load_policies(vec!["permit(principal, action, resource);".to_string()])?;
//! engine.register_entity(&user)?;
//!
//! let request = EngineRequest::new(&user, "Read", &document);
//! let decision = engine.is_authorized(&request)?;
//! ```

// New authorization engine (ALWAYS available - agnostic API)
pub mod engine;

// Legacy DI helpers (gated behind feature flag during migration)
#[cfg(feature = "legacy_infra")]
pub mod di_helpers;

// Re-export commonly used types from engine
pub use engine::{AuthorizationDecision, AuthorizationEngine, EngineError, EngineRequest};

// Legacy exports (behind feature flag)
#[cfg(feature = "legacy_infra")]
pub use di_helpers::*;
