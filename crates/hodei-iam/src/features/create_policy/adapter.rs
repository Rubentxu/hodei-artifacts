//! Adapter implementations for the create_policy feature
//!
//! This module provides concrete implementations of the ports defined for
//! policy management, including validation and persistence adapters.

use crate::features::create_policy::ports::{
    PolicyValidationError, PolicyValidator, ValidationError, ValidationResult, ValidationWarning,
};
use async_trait::async_trait;
use policies::features::validate_policy::{ValidatePolicyQuery, ValidatePolicyUseCase};
use std::sync::Arc;

/// Adapter that implements PolicyValidator using the policies crate's validation service
///
/// This adapter delegates policy validation to the policies crate, maintaining
/// the architectural boundary and keeping Cedar-specific logic isolated.
///
/// # Architecture
/// - This adapter lives in the infrastructure layer
/// - It translates between hodei-iam's domain concepts and the policies crate's API
/// - No direct Cedar dependencies in hodei-iam
pub struct PoliciesValidatorAdapter {
    validate_policy_use_case: Arc<ValidatePolicyUseCase>,
}

impl PoliciesValidatorAdapter {
    /// Create a new adapter instance
    ///
    /// # Arguments
    /// * `validate_policy_use_case` - The validation use case from the policies crate
    pub fn new(validate_policy_use_case: Arc<ValidatePolicyUseCase>) -> Self {
        Self {
            validate_policy_use_case,
        }
    }
}

#[async_trait]
impl PolicyValidator for PoliciesValidatorAdapter {
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        // Build query for the policies crate
        let query = ValidatePolicyQuery::new(policy_content.to_string());

        // Delegate validation to policies crate
        let result = self
            .validate_policy_use_case
            .execute(&query)
            .await
            .map_err(|e| PolicyValidationError::ServiceError(e.to_string()))?;

        // Convert the result to our domain types
        Ok(ValidationResult {
            is_valid: result.is_valid,
            errors: result
                .errors
                .into_iter()
                .map(|e| ValidationError {
                    message: e.message,
                    line: e.line,
                    column: e.column,
                })
                .collect(),
            warnings: result
                .warnings
                .into_iter()
                .map(|w| ValidationWarning {
                    message: w.message,
                    severity: w.severity,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would go here, testing the adapter with a real
    // ValidatePolicyUseCase instance. For now, unit tests will use mocks.
}
