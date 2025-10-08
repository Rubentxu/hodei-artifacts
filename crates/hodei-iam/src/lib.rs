//! # hodei-iam
//!
//! IAM (Identity and Access Management) Bounded Context for Hodei Artifacts.
//!
//! This crate provides IAM functionality following **Vertical Slice Architecture (VSA)**
//! with **Clean Architecture** principles.
//!
//! ## Architecture Principles
//!
//! - **Encapsulation**: Internal domain entities are NOT exposed publicly
//! - **Vertical Slice Architecture**: Each feature is self-contained
//! - **Interface Segregation**: Each feature defines minimal, specific ports
//!
//! ## Public API
//!
//! This crate exposes use cases (features) and infrastructure adapters.
//!
//! ## Usage Example
//!
//! ```ignore
//! use hodei_iam::{CreateUserUseCase, CreateUserCommand};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Use cases are injected with their dependencies through DI
//! let use_case = CreateUserUseCase::new(/* dependencies */);
//!
//! let command = CreateUserCommand {
//!     name: "Alice".to_string(),
//!     email: "alice@example.com".to_string(),
//!     tags: vec![],
//! };
//!
//! let result = use_case.execute(command).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Internal Structure (NOT PUBLIC)
//!
//! The `internal` module contains:
//! - Domain entities (User, Group)
//!
//! These are implementation details and should NOT be accessed directly.
//! All interactions must go through the public use case APIs.

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

/// Internal domain models (sealed, not public)
pub(crate) mod internal;

/// Feature modules following VSA
pub mod features;

/// Infrastructure implementations (public for DI)
pub mod infrastructure;

// ============================================================================
// PUBLIC API SURFACE
// ============================================================================

/// Public API re-exports
pub mod api;

pub use api::*;
