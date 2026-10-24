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
/// full persistence logic. The schema it receives is the one the read paths
/// produced, so a policy is checked against what the schema really declares.
pub struct PolicyValidatorAdapter;

impl PolicyValidatorAdapter {
    /// Create a new policy validator adapter
    pub fn new() -> Self {
        Self
    }

    /// Validate a single policy text against a schema
    ///
    /// The schema is applied with Cedar's own validator, so a policy is
    /// checked against what the schema really declares.
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
        schema: &Schema,
    ) -> Result<Vec<String>, PlaygroundEvaluateError> {
        debug!("Validating policy");

        // Parse the policy first
        let policy = cedar_policy::Policy::from_str(policy_text).map_err(|e| {
            warn!("Policy parsing failed: {}", e);
            PlaygroundEvaluateError::PolicyError(format!("Policy parse error: {}", e))
        })?;

        // A schema that declares no entity type and no action cannot approve
        // anything: applied as it is, it would report every policy that names
        // a type as invalid. That is a false rejection, not a validation. So
        // the schema is applied when it declares something, and only the
        // syntax is checked when it declares nothing — the same rule the
        // validate-policy use case applies when no schema is stored.
        if !declares_anything(schema) {
            debug!("The schema declares nothing, validating syntax only");
            return Ok(vec![]);
        }

        let policy_set = cedar_policy::PolicySet::from_policies(vec![policy]).map_err(|e| {
            warn!("Policy set could not be built: {}", e);
            PlaygroundEvaluateError::PolicyError(format!("Policy set error: {}", e))
        })?;

        let validation = cedar_policy::Validator::new(schema.clone())
            .validate(&policy_set, cedar_policy::ValidationMode::default());

        let errors: Vec<String> = validation
            .validation_errors()
            .map(|e| e.to_string())
            .collect();

        if errors.is_empty() {
            debug!("Policy validated against the schema");
        } else {
            warn!(
                error_count = errors.len(),
                "Policy failed schema validation"
            );
        }

        Ok(errors)
    }
}

impl Default for PolicyValidatorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the schema declares something a policy can be checked against.
///
/// A schema with no entity type and no action says nothing about the world,
/// so it cannot tell a valid policy from an invalid one.
fn declares_anything(schema: &Schema) -> bool {
    schema.entity_types().count() > 0 || schema.actions().count() > 0
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

    /// A schema that declares `Iam::User`, as the IAM schema does.
    fn schema_declaring_user() -> Schema {
        let fragment = cedar_policy::SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

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

    /// A policy over a declared type is accepted
    ///
    /// The validator used to ignore the schema, so this is the same result
    /// it gave before. What changed is the counterpart below: the schema is
    /// no longer ignored, so the acceptance means something.
    #[tokio::test]
    async fn test_policy_over_a_declared_type_is_accepted() {
        let validator = PolicyValidatorAdapter::new();
        let schema = schema_declaring_user();
        let policies =
            vec!["permit(principal == Iam::User::\"alice\", action, resource);".to_string()];

        let errors = validator
            .validate_policies(&policies, &schema)
            .await
            .expect("the validation must not fail");

        assert!(
            errors.is_empty(),
            "a policy over a declared entity type must be accepted: {errors:?}"
        );
    }

    /// A policy over an undeclared type is rejected
    ///
    /// This is the discriminating counterpart of the test above: with the
    /// schema applied, the rejection is about the undeclared type. Without
    /// it — with the schema ignored, or replaced by an empty one — the same
    /// policy was reported as valid.
    #[tokio::test]
    async fn test_policy_over_an_undeclared_type_is_rejected() {
        let validator = PolicyValidatorAdapter::new();
        let schema = schema_declaring_user();
        let policies =
            vec!["permit(principal == Nope::Ghost::\"alice\", action, resource);".to_string()];

        let errors = validator
            .validate_policies(&policies, &schema)
            .await
            .expect("the validation must not fail");

        assert!(
            !errors.is_empty(),
            "a policy over an undeclared entity type must be rejected"
        );
        assert!(
            errors[0].contains("Nope::Ghost"),
            "the rejection must name the undeclared type: {errors:?}"
        );
    }

    /// A schema that declares nothing leaves the syntax as the only check
    ///
    /// An empty schema applied as it is would report every policy that names
    /// a type as invalid, which is a false rejection: the schema says
    /// nothing about that type.
    #[tokio::test]
    async fn test_empty_schema_validates_syntax_only() {
        let validator = PolicyValidatorAdapter::new();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies =
            vec!["permit(principal == Iam::User::\"alice\", action, resource);".to_string()];

        let errors = validator
            .validate_policies(&policies, &schema)
            .await
            .expect("the validation must not fail");

        assert!(
            errors.is_empty(),
            "a schema that declares nothing must not reject a policy: {errors:?}"
        );
    }
}
