use crate::features::validate_policy::dto::{ValidatePolicyCommand, ValidationResult};
use crate::features::validate_policy::error::ValidatePolicyError;
use crate::features::validate_policy::port::ValidatePolicyPort;
use async_trait::async_trait;
use tracing::{info, warn};

pub struct ValidatePolicyUseCase;

impl Default for ValidatePolicyUseCase {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidatePolicyUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        command: ValidatePolicyCommand,
    ) -> Result<ValidationResult, ValidatePolicyError> {
        self.validate(command).await
    }
}

#[async_trait]
impl ValidatePolicyPort for ValidatePolicyUseCase {
    async fn validate(&self, command: ValidatePolicyCommand) -> Result<ValidationResult, ValidatePolicyError> {
        info!("Validating policy syntax");

        // Validate input: check if content is empty or whitespace
        let content = command.content.trim();
        if content.is_empty() {
            warn!("Policy content is empty");
            return Ok(ValidationResult {
                is_valid: false,
                errors: vec!["Policy content cannot be empty".to_string()],
            });
        }

        // Parse the policy using Cedar
        match cedar_policy::Policy::parse(None, content) {
            Ok(_) => {
                info!("Policy syntax is valid");
                Ok(ValidationResult {
                    is_valid: true,
                    errors: vec![],
                })
            }
            Err(e) => {
                warn!("Policy syntax validation failed: {:?}", e);
                // Format Cedar errors into Vec<String>
                let errors = format_cedar_errors(e);
                Ok(ValidationResult {
                    is_valid: false,
                    errors,
                })
            }
        }
    }
}

fn format_cedar_errors(error: cedar_policy::ParseErrors) -> Vec<String> {
    // Cedar errors can be complex; for now, convert to string representation
    // In a real implementation, you might want to parse the error structure more carefully
    vec![error.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_valid_policy_returns_is_valid_true() {
        let use_case = ValidatePolicyUseCase::new();
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
        };
        let result = use_case.execute(command).await.unwrap();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_policy_returns_is_valid_false_with_errors() {
        let use_case = ValidatePolicyUseCase::new();
        let command = ValidatePolicyCommand {
            content: "permit(principal, action);".to_string(),
        }; // Sintaxis incorrecta
        let result = use_case.execute(command).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("resource") || result.errors[0].contains("missing"));
    }

    #[tokio::test]
    async fn test_empty_policy_is_invalid() {
        let use_case = ValidatePolicyUseCase::new();
        let command = ValidatePolicyCommand {
            content: "   ".to_string(),
        };
        let result = use_case.execute(command).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors[0], "Policy content cannot be empty");
    }
}
