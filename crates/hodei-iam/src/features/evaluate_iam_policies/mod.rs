/// Feature: Evaluate IAM Policies
///
/// This feature evaluates IAM policies to determine if a principal has permission
/// to perform a specific action on a resource.
pub mod adapter;
pub mod di;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;
// TODO: REFACTOR (Phase 2) - use_case_test.rs needs to be updated after refactoring
// pub mod use_case_test;

pub use use_case::EvaluateIamPoliciesUseCase;
