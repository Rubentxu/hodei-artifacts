use async_trait::async_trait;
use crate::features::validate_policy::dto::ValidatePolicyCommand;
use crate::features::validate_policy::dto::ValidationResult;
use crate::features::validate_policy::error::ValidatePolicyError;

#[async_trait]
pub trait ValidatePolicyPort: Send + Sync {
    async fn validate(&self, command: ValidatePolicyCommand) -> Result<ValidationResult, ValidatePolicyError>;
}
