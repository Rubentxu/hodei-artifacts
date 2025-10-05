//! Infrastructure layer for the policies crate
//!
//! This module contains infrastructure implementations that interact with
//! external systems or provide technical utilities.
//!
//! ## Modules
//!
//! - `translator` - Translation layer from kernel's agnostic types to Cedar types (CRITICAL)
//! - `validator` - Policy syntax validation utility (Cedar DSL validation)
//! - `surreal` - SurrealDB adapters (legacy, gated behind feature flags)
//!
//! ## Design Notes
//!
//! The infrastructure layer should NOT contain business logic.
//! It only provides technical implementations of ports defined in the application layer.
//!
//! The `translator` module is the most critical component as it encapsulates Cedar
//! as an implementation detail, allowing the rest of the system to work with
//! agnostic types from the kernel crate.

// Translator - Cedar type translation (CRITICAL COMPONENT)
// This is the bridge between kernel's agnostic types and Cedar's specific types
pub mod translator;

// Policy validator utility
// This is kept as a shared utility since all domains that manage policies
// may want to validate Cedar DSL syntax before persisting
pub mod validator;

// SurrealDB infrastructure (legacy, mostly empty after cleanup)
pub mod surreal;
