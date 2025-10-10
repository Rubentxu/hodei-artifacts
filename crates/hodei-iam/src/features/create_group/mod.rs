//! Create group feature module
//!
//! This module implements the vertical slice for creating new groups.
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
pub use dto::{CreateGroupCommand, GroupView};
pub use error::CreateGroupError;
pub use use_case::CreateGroupUseCase;

// Export mocks for testing
#[cfg(test)]
pub use mocks::{MockCreateGroupPort, MockHrnGenerator};
