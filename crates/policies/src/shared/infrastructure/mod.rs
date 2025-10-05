//! Infrastructure layer for the policies crate
//!
//! This module contains infrastructure implementations that interact with
//! external systems or provide technical utilities.
//!
//! ## Modules
//!
//! - `surreal` - SurrealDB adapters (legacy, gated behind feature flags)
//! - `validator` - Policy syntax validation utility (Cedar DSL validation)
//!
//! ## Design Notes
//!
//! The infrastructure layer should NOT contain business logic.
//! It only provides technical implementations of ports defined in the application layer.

// SurrealDB infrastructure (legacy, mostly empty after cleanup)
pub mod surreal;

// Policy validator utility
// This is kept as a shared utility since all domains that manage policies
// may want to validate Cedar DSL syntax before persisting
pub mod validator;
