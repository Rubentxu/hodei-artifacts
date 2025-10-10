/// Feature: Evaluate IAM Policies
///
/// This feature evaluates IAM policies to determine if a principal has permission
/// to perform a specific action on a resource.
pub mod adapter;

pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;
#[cfg(test)]
mod use_case_test;

pub use use_case::EvaluateIamPoliciesUseCase;
