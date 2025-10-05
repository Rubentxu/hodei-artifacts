//! # Policies Crate - Authorization Engine Library
//!
//! This crate provides a **pure evaluation engine** for Cedar policies.
//! It is a library that encapsulates the Cedar policy engine as an implementation detail.
//!
//! ## Architecture Principles
//!
//! 1. **No Policy Management**: This crate does NOT manage policies (CRUD operations).
//!    Policy management is the responsibility of domain crates:
//!    - `hodei-iam` manages IAM policies
//!    - `hodei-organizations` manages Service Control Policies (SCPs)
//!
//! 2. **Pure Evaluation**: Only provides policy evaluation capabilities via `AuthorizationEngine`.
//!
//! 3. **Cedar Encapsulation**: Cedar is completely encapsulated. External crates interact
//!    only with agnostic types from the `kernel` crate.
//!
//! 4. **Translation Layer**: Provides internal translation from kernel's agnostic types
//!    (`HodeiEntity`, `AttributeValue`) to Cedar types (`Entity`, `RestrictedExpression`).
//!
//! ## Current State (During Refactor)
//!
//! The authorization engine is currently behind the `legacy_infra` feature flag while
//! it is being refactored to use agnostic types. The new agnostic API will be available
//! in the `shared::application::engine` module.
//!
//! ## Public API (Future - After Refactor)
//!
//! ```rust,ignore
//! use policies::shared::application::engine::{AuthorizationEngine, EngineRequest};
//! use kernel::{HodeiEntity, AttributeValue};
//!
//! // Create engine
//! let engine = AuthorizationEngine::new(/* schema */);
//!
//! // Evaluate with agnostic types (NO Cedar types exposed)
//! let request = EngineRequest {
//!     principal: &user,  // &dyn HodeiEntity
//!     action: "read",
//!     resource: &document,  // &dyn HodeiEntity
//!     context: HashMap::new(),
//! };
//!
//! let allowed = engine.is_authorized(request)?;
//! ```
//!
//! ## Module Structure
//!
//! - `shared::application::engine` - Authorization engine (public API, behind `legacy_infra` flag)
//! - `shared::infrastructure::translator` - Agnostic-to-Cedar translation (TO BE IMPLEMENTED)
//! - `shared::infrastructure::validator` - Policy syntax validation utility
//!
//! ## Feature Flags
//!
//! - `mem` - In-memory SurrealDB backend (default)
//! - `embedded` - Embedded RocksDB backend
//! - `legacy_infra` - Legacy infrastructure including current AuthorizationEngine
//!
//! ## Migration Status
//!
//! ✅ Policy management features removed (moved to domain crates)
//! ⏳ AuthorizationEngine refactor in progress (adding agnostic API)
//! ⏳ Translator implementation pending
//!
//! ## Design Notes
//!
//! This crate follows the principle of **encapsulation of external dependencies**.
//! Cedar is an implementation detail that can be replaced in the future without
//! affecting other crates, as long as the public API (agnostic types) is maintained.

pub mod shared;

// Re-export application layer (conditionally based on feature flags)
#[cfg(feature = "legacy_infra")]
pub use shared::application;

// Re-export infrastructure utilities (always available)
pub use shared::infrastructure;

// For backward compatibility during migration
// This alias will be removed once all dependent crates are updated
#[deprecated(
    since = "0.2.0",
    note = "Use specific imports from `shared` instead. The `domain` alias will be removed."
)]
pub use shared as domain;
