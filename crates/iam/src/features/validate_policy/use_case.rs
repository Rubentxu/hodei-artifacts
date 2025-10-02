use crate::features::validate_policy::dto::{ValidatePolicyRequest, ValidatePolicyResponse};
use crate::features::validate_policy::error::ValidatePolicyError;
use crate::features::validate_policy::ports::PolicyValidatorPort;
use std::sync::Arc;

// The use case for validating a policy.
pub struct ValidatePolicyUseCase {
    validator: Arc<dyn PolicyValidatorPort>,
}

impl ValidatePolicyUseCase {
    pub fn new(validator: Arc<dyn PolicyValidatorPort>) -> Self {
        Self { validator }
    }

    pub async fn execute(
        &self,
        request: ValidatePolicyRequest,
    ) -> Result<ValidatePolicyResponse, ValidatePolicyError> {
        // Here you could add more logic, like metrics, logging, etc.
        tracing::info!("Validating policy...");

        self.validator
            .validate_policy(&request.policy)
            .await
            .map_err(|e| {
                ValidatePolicyError::validation_failed(vec![
                    crate::infrastructure::errors::ValidationError {
                        message: format!("Policy validation failed: {}", e),
                        line: None,
                        column: None,
                    },
                ])
            })?;

        tracing::info!("Policy is valid.");
        Ok(ValidatePolicyResponse {
            is_valid: true,
            errors: vec![],
        })
    }
}
