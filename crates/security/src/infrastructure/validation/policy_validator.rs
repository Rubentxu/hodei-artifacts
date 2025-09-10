// crates/security/src/infrastructure/validation/policy_validator.rs

use crate::infrastructure::errors::SecurityError;
use cedar_policy::{Schema, PolicySet, Validator, ValidationMode};
use std::str::FromStr;
use std::sync::Arc;
use std::path::Path;
use async_trait::async_trait;

/// Validation result for policy validation
#[derive(Debug, Clone)]
pub struct PolicyValidationResult {
    pub is_valid: bool,
    pub errors: Vec<PolicyValidationError>,
    pub warnings: Vec<PolicyValidationWarning>,
}

/// Policy validation error
#[derive(Debug, Clone)]
pub struct PolicyValidationError {
    pub message: String,
    pub policy_id: Option<String>,
    pub location: Option<PolicyLocation>,
    pub error_type: ValidationErrorType,
}

/// Policy validation warning
#[derive(Debug, Clone)]
pub struct PolicyValidationWarning {
    pub message: String,
    pub policy_id: Option<String>,
    pub location: Option<PolicyLocation>,
}

/// Location information for validation errors
#[derive(Debug, Clone)]
pub struct PolicyLocation {
    pub line: u32,
    pub column: u32,
}

/// Types of validation errors
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationErrorType {
    SyntaxError,
    SemanticError,
    UnknownEntity,
    UnknownAction,
    UnknownAttribute,
    TypeMismatch,
    InvalidReference,
}

/// Policy validator with schema-based semantic validation
pub struct PolicyValidator {
    schema: Arc<Schema>,
}

impl PolicyValidator {
    /// Create a new policy validator by loading the schema from the given path.
    pub fn new(schema_path: &Path) -> Result<Self, SecurityError> {
        let schema_content = std::fs::read_to_string(schema_path)
            .map_err(|e| SecurityError::SchemaError(format!("Failed to read schema file: {} - path: {:?}", e, schema_path)))?;
        
        println!("DEBUG: Schema content preview: {}", &schema_content[..std::cmp::min(200, schema_content.len())]);
        
        let schema = Schema::from_str(&schema_content)
            .map_err(|e| SecurityError::SchemaError(format!("Failed to parse schema: {} - content preview: {}", e, &schema_content[..std::cmp::min(200, schema_content.len())])))?;

        Ok(Self { schema: Arc::new(schema) })
    }

    /// Validate a policy string with both syntax and semantic validation using the loaded schema.
    pub async fn validate_policy(&self, policy_content: &str) -> Result<PolicyValidationResult, SecurityError> {
        if policy_content.trim().is_empty() {
            return Ok(PolicyValidationResult {
                is_valid: false,
                errors: vec![PolicyValidationError {
                    message: "Policy content cannot be empty".to_string(),
                    policy_id: None,
                    location: None,
                    error_type: ValidationErrorType::SyntaxError,
                }],
                warnings: vec![],
            });
        }

        // First, parse the policy to check for syntax errors.
        let policy_set = match PolicySet::from_str(policy_content) {
            Ok(pset) => pset,
            Err(e) => {
                let error_message = e.to_string();
                let error = PolicyValidationError {
                    message: error_message.clone(),
                    policy_id: None,
                    location: Self::extract_location_from_error(&error_message),
                    error_type: ValidationErrorType::SyntaxError,
                };
                return Ok(PolicyValidationResult::invalid(vec![error]));
            }
        };

        // Now, validate the parsed policy set against the schema for semantic errors.
        let validator = Validator::new(self.schema.as_ref().clone());
        let validation_result = validator.validate(&policy_set, ValidationMode::default());

        if validation_result.validation_passed() {
            return Ok(PolicyValidationResult::valid());
        }

        let errors: Vec<_> = validation_result.validation_errors().map(|err| {
            let err_str = err.to_string();
            let error_type = Self::classify_error(&err_str);
            PolicyValidationError {
                message: err_str,
                policy_id: Some(err.policy_id().to_string()),
                location: None, // Note: Location is not easily available on validation errors yet.
                error_type,
            }
        }).collect();

        Ok(PolicyValidationResult::invalid(errors))
    }

    /// Extract location information from error messages
    fn extract_location_from_error(error_msg: &str) -> Option<PolicyLocation> {
        // This is a best-effort extraction. A more robust solution might involve a custom error parser.
        let re = regex::Regex::new(r"\(line (\d+), column (\d+)\)").ok()?;
        let caps = re.captures(error_msg)?;
        let line = caps.get(1)?.as_str().parse().ok()?;
        let column = caps.get(2)?.as_str().parse().ok()?;
        Some(PolicyLocation { line, column })
    }

    /// Classify the type of validation error based on the error message
    fn classify_error(error_msg: &str) -> ValidationErrorType {
        let msg_lower = error_msg.to_lowercase();
        
        if msg_lower.contains("unexpected token") || msg_lower.contains("parse error") {
            ValidationErrorType::SyntaxError
        } else if msg_lower.contains("undefined entity type") {
            ValidationErrorType::UnknownEntity
        } else if msg_lower.contains("unrecognized action") {
            ValidationErrorType::UnknownAction
        } else if msg_lower.contains("expected type") {
            ValidationErrorType::TypeMismatch
        } else {
            ValidationErrorType::SemanticError
        }
    }

    /// Get information about the validator
    pub fn info(&self) -> String {
        "PolicyValidator - Full syntax and semantic validation enabled.".to_string()
    }
}

/// Trait for policy validation services
#[async_trait]
pub trait PolicyValidationService: Send + Sync {
    async fn validate(&self, policy_content: &str) -> Result<PolicyValidationResult, SecurityError>;
}

#[async_trait]
impl PolicyValidationService for PolicyValidator {
    async fn validate(&self, policy_content: &str) -> Result<PolicyValidationResult, SecurityError> {
        self.validate_policy(policy_content).await
    }
}

impl PolicyValidationResult {
    /// Create a valid result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
        }
    }

    /// Create an invalid result with errors
    pub fn invalid(errors: Vec<PolicyValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn get_test_validator() -> PolicyValidator {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("schema/policy_schema.cedarschema");
        PolicyValidator::new(&path).expect("Failed to create validator for tests")
    }

    #[tokio::test]
    async fn test_validate_empty_policy() {
        let validator = get_test_validator();
        let result = validator.validate_policy("").await.unwrap();
        
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("empty"));
    }

    #[tokio::test]
    async fn test_validate_valid_policy() {
        let validator = get_test_validator();
        let policy = r#"permit(principal == Test::User::"alice", action == Test::ReadRepository, resource == Test::Repository::"myrepo");"#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        assert!(result.is_valid, "Policy should be valid. Errors: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_validate_invalid_entity_type() {
        let validator = get_test_validator();
        let policy = r#"permit(principal == Test::NonExistent::"user", action, resource);"#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].error_type, ValidationErrorType::UnknownEntity);
    }

    #[tokio::test]
    async fn test_validate_invalid_action() {
        let validator = get_test_validator();
        let policy = r#"permit(principal, action == Test::Action::"UnknownAction", resource);"#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].error_type, ValidationErrorType::UnknownAction);
    }

    #[tokio::test]
    async fn test_validate_syntax_error() {
        let validator = get_test_validator();
        let policy = r#"permit(principal, action, resource"#.to_string(); // Missing closing parenthesis and semicolon
        
        let result = validator.validate_policy(&policy).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors[0].error_type, ValidationErrorType::SyntaxError);
    }
}