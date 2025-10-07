//! Dependency injection configuration for the validate_policy feature

use crate::features::validate_policy::use_case::ValidatePolicyUseCase;

/// Factory for building the ValidatePolicyUseCase with its dependencies
pub struct ValidatePolicyUseCaseFactory;

impl ValidatePolicyUseCaseFactory {
    /// Build a new ValidatePolicyUseCase with default dependencies
    ///
    /// Since the use case has no external dependencies, this is a simple factory.
    pub fn build() -> ValidatePolicyUseCase {
        ValidatePolicyUseCase::new()
    }
}
