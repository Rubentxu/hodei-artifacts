/// Feature: Create Group
///
/// This feature allows creating new groups in the IAM system
pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

pub use dto::{CreateGroupCommand, GroupView};
pub use error::CreateGroupError;
pub use use_case::CreateGroupUseCase;
