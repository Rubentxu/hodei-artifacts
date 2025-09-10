use std::sync::Arc;
use crate::features::validate_policy::dto::{ValidatePolicyRequest, ValidatePolicyResponse};
use crate::features::validate_policy::ports::{PolicyValidatorPort, ValidationError};

// The use case for validating a policy.
pub struct ValidatePolicyUseCase {
    validator: Arc<dyn PolicyValidatorPort>,
}

impl ValidatePolicyUseCase {
    pub fn new(validator: Arc<dyn PolicyValidatorPort>) -> Self {
        Self { validator }
    }

    pub async fn execute(&self, request: ValidatePolicyRequest) -> Result<ValidatePolicyResponse, ValidationError> {
        // Here you could add more logic, like metrics, logging, etc.
        tracing::info!("Validating policy...");

        match self.validator.validate_policy(&request.policy).await {
            Ok(_) => {
                tracing::info!("Policy is valid.");
                Ok(ValidatePolicyResponse { is_valid: true, errors: vec![] })
            }
            Err(e) => {
                tracing::warn!("Policy validation failed: {}", e);
                Ok(ValidatePolicyResponse { is_valid: false, errors: vec![e.to_string()] })
            }
        }
    }
}