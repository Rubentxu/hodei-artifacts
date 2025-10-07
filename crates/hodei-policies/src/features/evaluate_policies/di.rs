//! Dependency Injection for the evaluate_policies feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::use_case::EvaluatePoliciesUseCase;

/// Factory for creating EvaluatePoliciesUseCase instances
pub struct EvaluatePoliciesUseCaseFactory;

impl EvaluatePoliciesUseCaseFactory {
    /// Creates a new EvaluatePoliciesUseCase instance
    ///
    /// Since the use case has no external dependencies, this is a simple factory.
    pub fn build() -> EvaluatePoliciesUseCase {
        EvaluatePoliciesUseCase::new()
    }
}
