use crate::features::validate_policy::ports::{PolicyValidatorPort, ValidationError};
use async_trait::async_trait;
use security::infrastructure::validation::PolicyValidator;
use std::path::PathBuf;
use std::sync::Arc;

// Adapter that implements the PolicyValidatorPort by calling the PolicyValidator from the security crate.
pub struct CedarValidatorAdapter {
    validator: Arc<PolicyValidator>,
}

impl CedarValidatorAdapter {
    pub fn new(schema_path: PathBuf) -> Result<Self, ValidationError> {
        let validator = PolicyValidator::new(&schema_path)
            .map_err(|e| ValidationError::Internal(e.to_string()))?;
        Ok(Self {
            validator: Arc::new(validator),
        })
    }
}

#[async_trait]
impl PolicyValidatorPort for CedarValidatorAdapter {
    async fn validate_policy(&self, policy: &str) -> Result<(), ValidationError> {
        let validator = self.validator.clone();
        let policy_str = policy.to_string();

        // The validation itself is CPU-bound, so we run it in a blocking task
        // to avoid blocking the async runtime.
        let result = tokio::task::spawn_blocking(move || {
            futures::executor::block_on(validator.validate_policy(&policy_str))
        })
        .await
        .map_err(|e| ValidationError::Internal(e.to_string()))?; // Task join error

        match result {
            Ok(validation_result) => {
                if validation_result.is_valid {
                    Ok(())
                } else {
                    // We take the first error for simplicity. A real implementation might handle multiple errors.
                    let first_error = validation_result
                        .errors
                        .get(0)
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown validation error".to_string());
                    Err(ValidationError::Semantic(first_error))
                }
            }
            Err(e) => Err(ValidationError::Internal(e.to_string())),
        }
    }
}
