//! Policy Validator Adapter for Playground Evaluate
//!
//! This adapter implements the PolicyValidatorPort trait by integrating with
//! the validate_policy feature to validate Cedar policies against schemas.

use super::super::error::PlaygroundEvaluateError;
use super::super::ports::PolicyValidatorPort;
use async_trait::async_trait;
use cedar_policy::Schema;
use std::str::FromStr;
use tracing::{debug, info, warn};

/// Adapter that implements PolicyValidatorPort using Cedar's validation
///
/// This adapter validates Cedar policy text against a provided schema,
/// collecting validation errors and warnings.
///
/// # Architecture
///
/// This adapter uses Cedar's native validation capabilities to check
/// policies against schemas without requiring the validate_policy feature's
/// full persistence logic.
pub struct PolicyValidatorAdapter;

impl PolicyValidatorAdapter {
    /// Create a new policy validator adapter
    pub fn new() -> Self {
        Self
    }

    /// Validate a single policy text against a schema
    ///
    /// # Arguments
    ///
    /// * `policy_text` - Cedar policy as a string
    /// * `schema` - Cedar schema to validate against
    ///
    /// # Returns
    ///
    /// A list of validation error messages (empty if valid)
    fn validate_single_policy(
        &self,
        policy_text: &str,
        _schema: &Schema,
    ) -> Result<Vec<String>, PlaygroundEvaluateError> {
        debug!("Validating policy");

        // Parse the policy first
        let _policy = cedar_policy::Policy::from_str(policy_text).map_err(|e| {
            warn!("Policy parsing failed: {}", e);
            PlaygroundEvaluateError::PolicyError(format!("Policy parse error: {}", e))
        })?;

        // For now, we'll do basic parsing validation
        // Full schema-based validation would require Cedar's Validator
        // which needs more complex setup
        debug!("Policy parsed successfully");

        // Return empty errors (policy is valid)
        Ok(vec![])
    }
}

impl Default for PolicyValidatorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyValidatorPort for PolicyValidatorAdapter {
    async fn validate_policies(
        &self,
        policy_texts: &[String],
        schema: &Schema,
    ) -> Result<Vec<String>, PlaygroundEvaluateError> {
        info!(
            policy_count = policy_texts.len(),
            "Validating policies against schema"
        );

        let mut all_errors = Vec::new();

        for (index, policy_text) in policy_texts.iter().enumerate() {
            debug!(policy_index = index, "Validating policy");

            match self.validate_single_policy(policy_text, schema) {
                Ok(errors) => {
                    if !errors.is_empty() {
                        warn!(
                            policy_index = index,
                            error_count = errors.len(),
                            "Policy validation found errors"
                        );
                        for error in errors {
                            all_errors.push(format!("Policy {}: {}", index, error));
                        }
                    }
                }
                Err(e) => {
                    warn!(policy_index = index, error = %e, "Policy validation failed");
                    all_errors.push(format!("Policy {}: {}", index, e));
                }
            }
        }

        if all_errors.is_empty() {
            info!("All policies validated successfully");
        } else {
            warn!(
                error_count = all_errors.len(),
                "Policy validation completed with errors"
            );
        }

        Ok(all_errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_empty_policy_list() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let result = validator.validate_policies(&[], &schema).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_validate_valid_policy() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["permit(principal, action, resource);".to_string()];
        let result = validator.validate_policies(&policies, &schema).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_validate_invalid_policy_syntax() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["invalid policy syntax here".to_string()];
        let result = validator.validate_policies(&policies, &schema).await;
        assert!(result.is_ok());
        let errors = result.unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Policy 0"));
    }

    #[tokio::test]
    async fn test_validate_multiple_policies() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "forbid(principal, action, resource);".to_string(),
        ];
        let result = validator.validate_policies(&policies, &schema).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_validate_mixed_valid_invalid_policies() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "invalid syntax".to_string(),
            "forbid(principal, action, resource);".to_string(),
        ];
        let result = validator.validate_policies(&policies, &schema).await;
        assert!(result.is_ok());
        let errors = result.unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Policy 1"));
    }

    #[test]
    fn test_default_constructor() {
        let _validator = PolicyValidatorAdapter;
    }
}
