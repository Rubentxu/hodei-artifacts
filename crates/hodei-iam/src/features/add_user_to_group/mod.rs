/// Feature: Add User to Group
///
/// This feature allows adding users to existing groups
pub mod adapter;
pub mod di;
pub mod dto;
pub mod ports;
pub mod use_case;

#[cfg(test)]
pub mod mocks;

#[cfg(test)]
mod use_case_test;

pub use use_case::AddUserToGroupUseCase;
