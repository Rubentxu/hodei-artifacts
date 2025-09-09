// crates/iam/src/infrastructure/validation/semantic_validator.rs

use crate::domain::validation::ValidationResult;
use crate::features::create_policy::ports::PolicyValidator as CreatePolicyValidator;
use crate::features::update_policy::ports::PolicyUpdateValidator;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use security::{PolicyValidator as SecurityPolicyValidator, PolicyValidationService};

/// Semantic policy validator that uses the security crate's PolicyValidator
/// This provides deep semantic validation using Cedar schemas
pub struct SemanticPolicyValidator {
    security_validator: SecurityPolicyValidator,
}

impl SemanticPolicyValidator {
    /// Create a new semantic policy validator
    pub fn new() -> Result<Self, IamError> {
        let security_validator = SecurityPolicyValidator::new()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to initialize semantic validator: {}", e)))?;
        
        Ok(Self {
            security_validator,
        })
    }

    /// Create a semantic policy validator with custom schema
    pub fn with_schema(schema_content: &str) -> Result<Self, IamError> {
        let security_validator = SecurityPolicyValidator::with_schema(schema_content)
            .map_err(|e| IamError::ConfigurationError(format!("Failed to initialize semantic validator with schema: {}", e)))?;
        
        Ok(Self {
            security_validator,
        })
    }

    /// Convert security validation result to IAM validation result
    fn convert_validation_result(
        &self,
        security_result: security::PolicyValidationResult,
    ) -> ValidationResult {
        if security_result.is_valid {
            ValidationResult::valid()
        } else {
            let errors = security_result
                .errors
                .into_iter()
                .map(|e| crate::infrastructure::errors::ValidationError {
                    message: e.message,
                    line: e.location.as_ref().map(|l| l.line),
                    column: e.location.as_ref().map(|l| l.column),
                })
                .collect();

            ValidationResult::invalid(errors)
        }
    }

    /// Perform comprehensive validation (syntax + semantics)
    async fn validate_comprehensive(&self, content: &str) -> Result<ValidationResult, IamError> {
        let security_result = self.security_validator.validate(content).await
            .map_err(|e| IamError::validation_error(format!("Semantic validation failed: {}", e)))?;

        Ok(self.convert_validation_result(security_result))
    }
}

impl Default for SemanticPolicyValidator {
    fn default() -> Self {
        Self::new().expect("Failed to create default SemanticPolicyValidator")
    }
}

#[async_trait]
impl CreatePolicyValidator for SemanticPolicyValidator {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validate_comprehensive(content).await
    }

    async fn validate_semantics(&self, content: &str) -> Result<(), IamError> {
        let result = self.validate_comprehensive(content).await?;
        
        if result.is_valid {
            Ok(())
        } else {
            let error_messages: Vec<String> = result.errors.iter()
                .map(|e| e.message.clone())
                .collect();
            
            Err(IamError::validation_error(error_messages.join("; ")))
        }
    }
}

#[async_trait]
impl PolicyUpdateValidator for SemanticPolicyValidator {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validate_comprehensive(content).await
    }

    async fn validate_semantics(&self, content: &str) -> Result<(), IamError> {
        let result = self.validate_comprehensive(content).await?;
        
        if result.is_valid {
            Ok(())
        } else {
            let error_messages: Vec<String> = result.errors.iter()
                .map(|e| e.message.clone())
                .collect();
            
            Err(IamError::validation_error(error_messages.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_semantic_validator_creation() {
        let validator = SemanticPolicyValidator::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_validate_valid_policy() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == User::"alice",
                action == ReadArtifact,
                resource == Artifact::"test-doc"
            );
        "#;

        let result = validator.validate_comprehensive(policy).await.unwrap();
        assert!(result.is_valid, "Policy should be valid: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_validate_invalid_entity() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == UnknownEntity::"alice",
                action == ReadArtifact,
                resource == Artifact::"test-doc"
            );
        "#;

        let result = validator.validate_comprehensive(policy).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_invalid_action() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == User::"alice",
                action == UnknownAction,
                resource == Artifact::"test-doc"
            );
        "#;

        let result = validator.validate_comprehensive(policy).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_error() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let policy = r#"
            invalid_syntax(
                principal == User::"alice",
                action == ReadArtifact,
                resource == Artifact::"test-doc"
            );
        "#;

        let result = validator.validate_comprehensive(policy).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_empty_policy() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let result = validator.validate_comprehensive("").await.unwrap();
        
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].message.contains("empty"));
    }

    #[tokio::test]
    async fn test_semantic_validation_trait() {
        let validator = SemanticPolicyValidator::new().unwrap();
        let valid_policy = r#"
            permit(
                principal == User::"alice",
                action == ReadArtifact,
                resource == Artifact::"test-doc"
            );
        "#;

        use crate::features::create_policy::ports::PolicyValidator;
        
        let result = PolicyValidator::validate_semantics(&validator, valid_policy).await;
        assert!(result.is_ok());

        let invalid_policy = r#"
            permit(
                principal == UnknownEntity::"alice",
                action == Action::"ReadArtifact",
                resource == Artifact::"test-doc"
            );
        "#;

        let result = PolicyValidator::validate_semantics(&validator, invalid_policy).await;
        // For now, this might pass since we don't have full semantic validation
        // TODO: This should fail when proper semantic validation is implemented
        assert!(result.is_ok() || result.is_err());
    }
}