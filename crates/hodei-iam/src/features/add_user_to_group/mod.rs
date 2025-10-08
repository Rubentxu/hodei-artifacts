//! Add user to group feature module
//!
//! This module implements the vertical slice for adding users to groups.
//! It follows the Clean Architecture and Vertical Slice Architecture patterns.

pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
pub mod di;
mod use_case_test;

// Re-export the main types for convenience
pub use dto::AddUserToGroupCommand;
pub use error::AddUserToGroupError;
pub use use_case::AddUserToGroupUseCase;