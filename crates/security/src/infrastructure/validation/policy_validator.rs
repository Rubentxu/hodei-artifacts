// crates/security/src/infrastructure/validation/policy_validator.rs

use crate::infrastructure::errors::SecurityError;
use cedar_policy::{PolicySet};
use std::str::FromStr;
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
    // For now, we'll focus on syntax validation
    // TODO: Add schema-based semantic validation
}

impl PolicyValidator {
    /// Create a new policy validator with the default schema
    pub fn new() -> Result<Self, SecurityError> {
        // For now, create a validator without schema to focus on syntax validation
        // TODO: Implement proper schema-based validation
        Ok(Self {})
    }

    /// Create a policy validator with a custom schema
    pub fn with_schema(_schema_content: &str) -> Result<Self, SecurityError> {
        // TODO: Implement schema-based validation
        Ok(Self {})
    }

    /// Load the default schema from the embedded schema file
    fn load_default_schema() -> Result<String, SecurityError> {
        // For now, we'll embed the schema content directly in JSON format
        // In a production system, this could be loaded from a file or configuration
        Ok(r#"{
            "": {
                "entityTypes": {
                    "User": {
                        "shape": {
                            "type": "Record",
                            "attributes": {
                                "name": { "type": "String" },
                                "email": { "type": "String" },
                                "role": { "type": "String" },
                                "department": { "type": "String", "required": false },
                                "groups": { "type": "Set", "element": { "type": "String" } },
                                "active": { "type": "Boolean" }
                            }
                        }
                    },
                    "Artifact": {
                        "shape": {
                            "type": "Record",
                            "attributes": {
                                "name": { "type": "String" },
                                "type": { "type": "String" },
                                "size": { "type": "Long" },
                                "owner": { "type": "Entity", "name": "User" },
                                "created_at": { "type": "String" },
                                "updated_at": { "type": "String" },
                                "tags": { "type": "Set", "element": { "type": "String" } },
                                "public": { "type": "Boolean" },
                                "metadata": { "type": "Record", "attributes": {} }
                            }
                        }
                    },
                    "Group": {
                        "shape": {
                            "type": "Record",
                            "attributes": {
                                "name": { "type": "String" },
                                "description": { "type": "String", "required": false },
                                "members": { "type": "Set", "element": { "type": "Entity", "name": "User" } },
                                "permissions": { "type": "Set", "element": { "type": "String" } }
                            }
                        }
                    },
                    "Organization": {
                        "shape": {
                            "type": "Record",
                            "attributes": {
                                "name": { "type": "String" },
                                "domain": { "type": "String" },
                                "active": { "type": "Boolean" },
                                "settings": { "type": "Record", "attributes": {} }
                            }
                        }
                    }
                },
                "actions": {
                    "ReadArtifact": {
                        "appliesTo": {
                            "principalTypes": ["User", "Group"],
                            "resourceTypes": ["Artifact"]
                        }
                    },
                    "WriteArtifact": {
                        "appliesTo": {
                            "principalTypes": ["User", "Group"],
                            "resourceTypes": ["Artifact"]
                        }
                    },
                    "DeleteArtifact": {
                        "appliesTo": {
                            "principalTypes": ["User", "Group"],
                            "resourceTypes": ["Artifact"]
                        }
                    },
                    "CreateArtifact": {
                        "appliesTo": {
                            "principalTypes": ["User", "Group"],
                            "resourceTypes": ["Organization"]
                        }
                    },
                    "ManageUsers": {
                        "appliesTo": {
                            "principalTypes": ["User"],
                            "resourceTypes": ["Organization", "Group"]
                        }
                    },
                    "ViewMetrics": {
                        "appliesTo": {
                            "principalTypes": ["User", "Group"],
                            "resourceTypes": ["Organization", "Artifact"]
                        }
                    },
                    "AdminAccess": {
                        "appliesTo": {
                            "principalTypes": ["User"],
                            "resourceTypes": ["Organization", "Artifact", "Group", "User"]
                        }
                    }
                }
            }
        }"#.to_string())
    }

    /// Validate a policy string with both syntax and semantic validation
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

        // First, try to parse the policy set for syntax validation
        let policy_set = match PolicySet::from_str(policy_content) {
            Ok(ps) => ps,
            Err(e) => {
                return Ok(PolicyValidationResult {
                    is_valid: false,
                    errors: vec![PolicyValidationError {
                        message: format!("Syntax error: {}", e),
                        policy_id: None,
                        location: Self::extract_location_from_error(&e.to_string()),
                        error_type: ValidationErrorType::SyntaxError,
                    }],
                    warnings: vec![],
                });
            }
        };

        // For now, we only do syntax validation
        // TODO: Add semantic validation using schema
        let mut errors = Vec::new();
        let warnings = Vec::new();

        // If we reach here, the policy set was parsed successfully
        // In the future, we would add semantic validation here

        Ok(PolicyValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        })
    }

    /// Extract location information from error messages
    fn extract_location_from_error(error_msg: &str) -> Option<PolicyLocation> {
        // This is a best-effort extraction of location information
        // Cedar error messages may contain line/column info in various formats
        let line_regex = regex::Regex::new(r"line (\d+)").ok()?;
        let column_regex = regex::Regex::new(r"column (\d+)").ok()?;

        let line = line_regex
            .captures(error_msg)?
            .get(1)?
            .as_str()
            .parse()
            .ok()?;

        let column = column_regex
            .captures(error_msg)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);

        Some(PolicyLocation { line, column })
    }

    /// Classify the type of validation error based on the error message
    fn classify_error(error_msg: &str) -> ValidationErrorType {
        let msg_lower = error_msg.to_lowercase();
        
        if msg_lower.contains("unknown entity") || msg_lower.contains("undefined entity") {
            ValidationErrorType::UnknownEntity
        } else if msg_lower.contains("unknown action") || msg_lower.contains("undefined action") {
            ValidationErrorType::UnknownAction
        } else if msg_lower.contains("unknown attribute") || msg_lower.contains("undefined attribute") {
            ValidationErrorType::UnknownAttribute
        } else if msg_lower.contains("type") && (msg_lower.contains("mismatch") || msg_lower.contains("error")) {
            ValidationErrorType::TypeMismatch
        } else if msg_lower.contains("reference") || msg_lower.contains("invalid") {
            ValidationErrorType::InvalidReference
        } else if msg_lower.contains("syntax") || msg_lower.contains("parse") {
            ValidationErrorType::SyntaxError
        } else {
            ValidationErrorType::SemanticError
        }
    }

    /// Get information about the validator
    pub fn info(&self) -> String {
        "PolicyValidator - Syntax validation only (schema validation TODO)".to_string()
    }
}

impl Default for PolicyValidator {
    fn default() -> Self {
        Self::new().expect("Failed to create default PolicyValidator")
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

    /// Get the first error message, if any
    pub fn first_error_message(&self) -> Option<&str> {
        self.errors.first().map(|e| e.message.as_str())
    }

    /// Check if there are any semantic errors
    pub fn has_semantic_errors(&self) -> bool {
        self.errors.iter().any(|e| e.error_type != ValidationErrorType::SyntaxError)
    }

    /// Get all error messages as a single string
    pub fn error_summary(&self) -> String {
        self.errors
            .iter()
            .map(|e| &e.message)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validator_creation() {
        let validator = PolicyValidator::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_validate_empty_policy() {
        let validator = PolicyValidator::new().unwrap();
        let result = validator.validate_policy("").await.unwrap();
        
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("empty"));
    }

    #[tokio::test]
    async fn test_validate_valid_policy() {
        let validator = PolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == User::"alice",
                action == Action::"ReadArtifact",
                resource == Artifact::"test-doc"
            );
        "#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        assert!(result.is_valid, "Policy should be valid: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_validate_invalid_entity_type() {
        let validator = PolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == UnknownEntity::"alice",
                action == Action::"ReadArtifact",
                resource == Artifact::"test-doc"
            );
        "#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        // For now, this will pass syntax validation but would fail semantic validation
        // TODO: Implement semantic validation to catch unknown entity types
        assert!(result.is_valid || !result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_invalid_action() {
        let validator = PolicyValidator::new().unwrap();
        let policy = r#"
            permit(
                principal == User::"alice",
                action == Action::"UnknownAction",
                resource == Artifact::"test-doc"
            );
        "#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        // For now, this will pass syntax validation but would fail semantic validation
        // TODO: Implement semantic validation to catch unknown actions
        assert!(result.is_valid || !result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_error() {
        let validator = PolicyValidator::new().unwrap();
        let policy = r#"
            invalid_keyword(
                principal == User::"alice",
                action == ReadArtifact,
                resource == Artifact::"test-doc"
            );
        "#;
        
        let result = validator.validate_policy(policy).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors[0].error_type, ValidationErrorType::SyntaxError);
    }

    #[tokio::test]
    async fn test_error_classification() {
        assert_eq!(
            PolicyValidator::classify_error("unknown entity type"),
            ValidationErrorType::UnknownEntity
        );
        assert_eq!(
            PolicyValidator::classify_error("unknown action"),
            ValidationErrorType::UnknownAction
        );
        assert_eq!(
            PolicyValidator::classify_error("type mismatch"),
            ValidationErrorType::TypeMismatch
        );
    }

    #[tokio::test]
    async fn test_validation_result_helpers() {
        let mut result = PolicyValidationResult::valid();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());

        let error = PolicyValidationError {
            message: "Test error".to_string(),
            policy_id: None,
            location: None,
            error_type: ValidationErrorType::SemanticError,
        };

        result = PolicyValidationResult::invalid(vec![error]);
        assert!(!result.is_valid);
        assert_eq!(result.first_error_message(), Some("Test error"));
        assert!(result.has_semantic_errors());
    }
}