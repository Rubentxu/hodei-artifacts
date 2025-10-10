//! Create user feature module
//!
//! This module implements the vertical slice for creating new users.
//! It follows the Clean Architecture and Vertical Slice Architecture patterns.

pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

// Re-export the main types for convenience
pub use dto::{CreateUserCommand, UserView};
pub use error::CreateUserError;
pub use use_case::CreateUserUseCase;
