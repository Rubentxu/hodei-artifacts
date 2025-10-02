/// Feature: Create Group
///
/// This feature allows creating new groups in the IAM system

pub mod dto;
pub mod use_case;
pub mod di;

pub use dto::{CreateGroupCommand, GroupView};
pub use use_case::CreateGroupUseCase;

