use crate::features::load_schema::ports::SchemaStoragePort;
use crate::features::validate_policy::dto::{ValidatePolicyCommand, ValidationResult};
use crate::features::validate_policy::error::ValidatePolicyError;
use crate::features::validate_policy::port::ValidatePolicyPort;
use async_trait::async_trait;
use cedar_policy::Schema;
use std::sync::Arc;
use tracing::{info, warn};

/// Use case for validating Cedar policies
///
/// This use case validates policy syntax and optionally validates
/// against a Cedar schema if a schema storage is provided.
pub struct ValidatePolicyUseCase<S: SchemaStoragePort> {
    /// Optional schema storage for schema-based validation
    schema_storage: Option<Arc<S>>,
}

impl<S: SchemaStoragePort> Default for ValidatePolicyUseCase<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: SchemaStoragePort> ValidatePolicyUseCase<S> {
    /// Create a new policy validation use case without schema validation
    pub fn new() -> Self {
        Self {
            schema_storage: None,
        }
    }

    /// Create a new policy validation use case with schema validation
    ///
    /// When schema storage is provided, the use case will attempt to load
    /// the latest schema and validate policies against it.
    pub fn with_schema_storage(schema_storage: Arc<S>) -> Self {
        Self {
            schema_storage: Some(schema_storage),
        }
    }

    pub async fn execute(
        &self,
        command: ValidatePolicyCommand,
    ) -> Result<ValidationResult, ValidatePolicyError> {
        self.validate(command).await
    }

    /// Load the latest schema from storage if available
    ///
    /// The three outcomes are kept apart on purpose:
    ///
    /// - `Ok(Some(schema))`: a schema was stored and could be applied.
    /// - `Ok(None)`: no schema has been stored at all, so only the syntax of
    ///   a policy can be checked.
    /// - `Err(..)`: a schema *is* stored but could not be applied. This is
    ///   never downgraded to "no schema": a policy that cannot be checked
    ///   against the schema must not be reported as valid.
    async fn load_schema(&self) -> Result<Option<Schema>, ValidatePolicyError> {
        if let Some(ref storage) = self.schema_storage {
            match storage.get_latest_schema().await {
                Ok(Some(schema_string)) => {
                    info!("Retrieved schema from storage for validation");
                    let schema = crate::internal::engine::schema_codec::decode(&schema_string)
                        .map_err(|e| {
                            ValidatePolicyError::ValidationError(format!(
                                "the stored schema could not be applied: {e}"
                            ))
                        })?;
                    Ok(Some(schema))
                }
                Ok(None) => {
                    warn!("No schema found in storage");
                    Ok(None)
                }
                Err(e) => Err(ValidatePolicyError::ValidationError(format!(
                    "the stored schema could not be read: {e}"
                ))),
            }
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl<S: SchemaStoragePort> ValidatePolicyPort for ValidatePolicyUseCase<S> {
    async fn validate(
        &self,
        command: ValidatePolicyCommand,
    ) -> Result<ValidationResult, ValidatePolicyError> {
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
        let policy = match cedar_policy::Policy::parse(None, content) {
            Ok(p) => {
                info!("Policy syntax is valid");
                p
            }
            Err(e) => {
                warn!("Policy syntax validation failed: {:?}", e);
                let errors = format_cedar_errors(e);
                return Ok(ValidationResult {
                    is_valid: false,
                    errors,
                });
            }
        };

        // If schema storage is available, validate against schema
        if self.schema_storage.is_some() {
            info!("Attempting schema-based validation");
            match self.load_schema().await? {
                Some(schema) => {
                    // Perform schema-based validation
                    // Create a PolicySet with a single policy
                    let policy_set = cedar_policy::PolicySet::from_policies(vec![policy])
                        .map_err(|e| ValidatePolicyError::ValidationError(e.to_string()))?;

                    let validation_result = cedar_policy::Validator::new(schema)
                        .validate(&policy_set, cedar_policy::ValidationMode::default());

                    // Check if there are validation errors
                    let validation_errors: Vec<String> = validation_result
                        .validation_errors()
                        .map(|e| e.to_string())
                        .collect();

                    if !validation_errors.is_empty() {
                        warn!("Policy failed schema validation");
                        return Ok(ValidationResult {
                            is_valid: false,
                            errors: validation_errors,
                        });
                    }

                    info!("Policy passed schema validation");
                }
                None => {
                    // No schema has been stored, so only the syntax of the
                    // policy could be checked. This is a real mode, not a
                    // degraded one: there is nothing to validate against.
                    info!("No schema stored, validating syntax only");
                }
            }
        }

        Ok(ValidationResult {
            is_valid: true,
            errors: vec![],
        })
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
    use crate::features::build_schema::error::BuildSchemaError;
    use crate::internal::engine::schema_codec;
    use async_trait::async_trait;
    use cedar_policy::SchemaFragment;

    /// Builds a *real* stored schema for the given entity type.
    ///
    /// The mock used to hand out the placeholder `"Schema(...)"`. The
    /// validator ignored the stored content and rebuilt an empty schema, so
    /// the test could not tell a working schema path from a broken one.
    fn stored_schema(entity_name: &str) -> String {
        let dsl = format!(
            r#"
namespace Iam {{
    entity {entity_name} {{
        name: String
    }};
}}
action "CreateUser" appliesTo {{
    principal: [Iam::{entity_name}],
    resource: [Iam::{entity_name}]
}};
"#
        );

        let fragment = SchemaFragment::from_cedarschema_str(&dsl)
            .expect("the fixture schema must parse")
            .0;

        schema_codec::encode(&[fragment]).expect("the fixture schema must be encodable")
    }

    // Mock storage for testing
    #[derive(Clone)]
    struct MockSchemaStorage {
        should_return_schema: bool,
    }

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            _schema_json: String,
            _version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            Ok("schema_1".to_string())
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            if self.should_return_schema {
                Ok(Some(stored_schema("User")))
            } else {
                Ok(None)
            }
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
            Ok(true)
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_valid_policy_returns_is_valid_true() {
        let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
        };
        let result = use_case.execute(command).await.unwrap();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_policy_returns_is_valid_false_with_errors() {
        let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
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
        let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
        let command = ValidatePolicyCommand {
            content: "   ".to_string(),
        };
        let result = use_case.execute(command).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors[0], "Policy content cannot be empty");
    }

    #[tokio::test]
    async fn test_validation_with_schema_storage() {
        let storage = Arc::new(MockSchemaStorage {
            should_return_schema: true,
        });
        let use_case = ValidatePolicyUseCase::with_schema_storage(storage);
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
        };

        let result = use_case.execute(command).await.unwrap();

        // The stored schema is applied, so a policy that references the
        // declared entity type passes and a policy that references an
        // undeclared one does not. The previous version of this test only
        // checked that a result came back at all.
        assert!(
            result.is_valid,
            "a policy over the declared entity type must be accepted: {:?}",
            result.errors
        );
    }

    /// A policy over an undeclared type must be rejected when a schema exists
    ///
    /// This is the discriminating counterpart: with the schema applied, the
    /// rejection is about the undeclared type and not about the validator
    /// rejecting everything.
    #[tokio::test]
    async fn test_validation_over_an_undeclared_type_is_rejected() {
        let storage = Arc::new(MockSchemaStorage {
            should_return_schema: true,
        });
        let use_case = ValidatePolicyUseCase::with_schema_storage(storage);
        let command = ValidatePolicyCommand {
            content: "permit(principal == Nope::Ghost::\"alice\", action, resource);".to_string(),
        };

        let result = use_case.execute(command).await.unwrap();

        assert!(
            !result.is_valid,
            "a policy over an undeclared entity type must be rejected: {:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn test_validation_without_schema_storage() {
        let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
        };
        let result = use_case.execute(command).await.unwrap();
        assert!(result.is_valid);
    }
}
