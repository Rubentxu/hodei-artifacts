use std::sync::Arc;

use cedar_policy::Policy;

use crate::shared::application::PolicyStore;
use super::dto::{ValidatePolicyQuery, ValidationError, ValidationResult};

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
}

pub struct ValidatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl ValidatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ValidatePolicyQuery,
    ) -> Result<ValidationResult, ValidatePolicyError> {
        // 1. Validar query
        query
            .validate()
            .map_err(|e| ValidatePolicyError::InvalidQuery(e.to_string()))?;

        // 2. Intentar parsear la política
        let policy_result: Result<Policy, _> = query.policy_content.parse();

        match policy_result {
            Ok(policy) => {
                // 3. Validar contra el schema usando el validator del store
                match self.store.validate_policy(&policy) {
                    Ok(()) => Ok(ValidationResult {
                        is_valid: true,
                        errors: vec![],
                        warnings: vec![],
                    }),
                    Err(e) => Ok(ValidationResult {
                        is_valid: false,
                        errors: vec![ValidationError {
                            message: e,
                            line: None,
                            column: None,
                        }],
                        warnings: vec![],
                    }),
                }
            }
            Err(e) => Ok(ValidationResult {
                is_valid: false,
                errors: vec![ValidationError {
                    message: format!("Parse error: {}", e),
                    line: None,
                    column: None,
                }],
                warnings: vec![],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;
    use std::sync::Arc;

    #[tokio::test]
    async fn validate_policy_accepts_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("permit(principal, action, resource);".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }

    #[tokio::test]
    async fn validate_policy_rejects_invalid_syntax() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query =
            ValidatePolicyQuery::new("this is not valid cedar syntax".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(!result.is_valid);
        assert!(result.errors.len() > 0);
        assert!(result.errors[0].message.contains("Parse error"));
    }

    #[tokio::test]
    async fn validate_policy_rejects_empty_content() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("".to_string());
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(ValidatePolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }

    #[tokio::test]
    async fn validate_policy_accepts_complex_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let complex_policy = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let query = ValidatePolicyQuery::new(complex_policy.to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }
}
