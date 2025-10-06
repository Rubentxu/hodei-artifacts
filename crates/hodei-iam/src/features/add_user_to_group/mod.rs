/// Feature: Add User to Group
///
/// This feature allows adding existing users to existing groups in the IAM system
pub mod adapter;
pub mod di;
pub mod dto;
pub mod mocks;
pub mod ports;
pub mod use_case;
// TODO: REFACTOR (Phase 2) - use_case_test.rs needs to be updated after refactoring
// pub mod use_case_test;

pub use dto::AddUserToGroupCommand;
pub use use_case::AddUserToGroupUseCase;
