//! Create user feature module
//!
//! This module implements the vertical slice for creating new users.
//! It follows the Clean Architecture and Vertical Slice Architecture patterns.

pub mod dto;
pub mod error;
pub mod factories;
pub mod ports;
pub mod use_case;
mod mocks;
#[cfg(test)]
mod use_case_test;

// Re-export the main types for convenience
pub use dto::{CreateUserCommand, UserView};
pub use error::CreateUserError;
pub use use_case::CreateUserUseCase;
