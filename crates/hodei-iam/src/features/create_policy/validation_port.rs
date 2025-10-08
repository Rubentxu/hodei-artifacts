use async_trait::async_trait;
use hodei_policies::features::validate_policy::{ValidatePolicyCommand, ValidationResult};
use super::error::CreatePolicyError;

#[async_trait]
pub trait PolicyValidator: Send + Sync {
    async fn validate(&self, content: &str) -> Result<ValidationResult, CreatePolicyError>;
}
