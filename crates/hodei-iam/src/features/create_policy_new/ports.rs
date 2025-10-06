//! Ports (interfaces) for the create_policy feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (ISP) from SOLID, each port is
//! specific and minimal - containing only the operations needed by this feature.
//!
//! # Architecture
//!
//! - `PolicyValidator`: Port for validating Cedar policy syntax
//! - `CreatePolicyPort`: Port for persisting policies (ONLY create operation)
//!
//! # ISP Compliance
//!
//! Note that `CreatePolicyPort` contains ONLY the `create` method, not update/delete/get/list.
//! This ensures that implementations and consumers of this port are not forced to depend
//! on operations they don't need.

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use async_trait::async_trait;
use policies::shared::domain::Policy;

/// Port for validating IAM policy content
///
/// This port abstracts policy validation, delegating to the policies crate
/// without creating a direct dependency on Cedar implementation details.
///
/// # Purpose
///
/// Validates that a Cedar policy text is syntactically and semantically correct
/// before it is persisted. This prevents storing invalid policies that would
/// cause runtime errors during authorization.
///
/// # Segregation
///
/// This port is segregated specifically for policy validation and does not
/// include persistence, evaluation, or other concerns.
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct CedarPolicyValidator {
///     // Cedar validator instance
/// }
///
/// #[async_trait]
/// impl PolicyValidator for CedarPolicyValidator {
///     async fn validate_policy(
///         &self,
///         policy_content: &str,
///     ) -> Result<ValidationResult, PolicyValidationError> {
///         // Parse with Cedar and return validation result
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyValidator: Send + Sync {
    /// Validate a Cedar policy content string
    ///
    /// # Arguments
    ///
    /// * `policy_content` - The Cedar policy text to validate
    ///
    /// # Returns
    ///
    /// A `ValidationResult` containing:
    /// - `is_valid`: Whether the policy is valid
    /// - `errors`: List of validation errors (if any)
    /// - `warnings`: List of non-blocking warnings
    ///
    /// # Errors
    ///
    /// Returns `PolicyValidationError::ServiceError` if the validation service
    /// itself fails (e.g., network error, service unavailable). This is different
    /// from the policy being invalid.
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError>;
}

/// Result of policy validation
///
/// This struct contains the outcome of validating a Cedar policy,
/// including any errors or warnings found.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the policy is syntactically and semantically valid
    pub is_valid: bool,

    /// List of validation errors that prevent the policy from being used
    pub errors: Vec<ValidationError>,

    /// List of warnings that don't prevent usage but should be addressed
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    /// Create a successful validation result with no errors
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
        }
    }

    /// Create a failed validation result with errors
    pub fn invalid(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: vec![],
        }
    }
}

/// A validation error with optional location information
///
/// Provides details about what is wrong with the policy,
/// including the location in the source text if available.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Human-readable error message
    pub message: String,

    /// Line number where the error occurred (1-based, optional)
    pub line: Option<usize>,

    /// Column number where the error occurred (1-based, optional)
    pub column: Option<usize>,
}

impl ValidationError {
    /// Create a new validation error with message only
    pub fn new(message: String) -> Self {
        Self {
            message,
            line: None,
            column: None,
        }
    }

    /// Create a validation error with location information
    pub fn with_location(message: String, line: usize, column: usize) -> Self {
        Self {
            message,
            line: Some(line),
            column: Some(column),
        }
    }
}

/// A validation warning
///
/// Warnings indicate potential issues that don't prevent the policy
/// from being used, but should be reviewed.
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// Human-readable warning message
    pub message: String,

    /// Severity level (e.g., "low", "medium", "high")
    pub severity: String,
}

/// Errors that can occur during policy validation
#[derive(Debug, thiserror::Error)]
pub enum PolicyValidationError {
    /// The validation service itself encountered an error
    ///
    /// This is different from the policy being invalid - it means
    /// the validation process couldn't complete.
    #[error("validation service error: {0}")]
    ServiceError(String),

    /// The validation service is temporarily unavailable
    #[error("validation service unavailable")]
    ServiceUnavailable,

    /// Timeout while waiting for validation result
    #[error("validation timeout after {0}ms")]
    Timeout(u64),
}

/// Port for creating IAM policies
///
/// This port defines the interface for persisting newly created policies.
///
/// # Interface Segregation Principle (ISP)
///
/// **IMPORTANT**: This trait contains ONLY the `create` operation.
/// It does NOT include update, delete, get, or list operations.
///
/// This segregation ensures:
/// - Implementations only need to support creation
/// - Consumers don't depend on unused operations
/// - Each operation can evolve independently
/// - Testing is simpler (smaller interface to mock)
///
/// # Why Segregated?
///
/// In the original monolithic `PolicyPersister` trait, all CRUD operations
/// were mixed together. This violated ISP because:
/// - A feature that only needs DELETE was forced to depend on CREATE, UPDATE, GET, LIST
/// - Mocks had to implement all 5 methods even if only testing 1
/// - Changes to one operation affected all consumers
///
/// By segregating into separate traits (`CreatePolicyPort`, `DeletePolicyPort`, etc.),
/// each feature can depend only on what it needs.
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct SurrealCreatePolicyAdapter {
///     db: SurrealClient,
/// }
///
/// #[async_trait]
/// impl CreatePolicyPort for SurrealCreatePolicyAdapter {
///     async fn create(
///         &self,
///         command: CreatePolicyCommand,
///     ) -> Result<Policy, CreatePolicyError> {
///         // Insert policy into SurrealDB
///         // Return created policy with metadata
///     }
/// }
/// ```
#[async_trait]
pub trait CreatePolicyPort: Send + Sync {
    /// Create a new policy and persist it
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy details (id, content, description)
    ///
    /// # Returns
    ///
    /// The created `Policy` entity with generated metadata (timestamps, HRN, etc.)
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::StorageError` - Database or storage failure
    /// - `CreatePolicyError::PolicyAlreadyExists` - Policy with this ID already exists
    /// - `CreatePolicyError::InvalidPolicyId` - Policy ID format is invalid
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = CreatePolicyCommand {
    ///     policy_id: "allow-read-docs".to_string(),
    ///     policy_content: "permit(...);".to_string(),
    ///     description: Some("Allow document reading".to_string()),
    /// };
    ///
    /// let policy = port.create(command).await?;
    /// println!("Created policy with HRN: {}", policy.id);
    /// ```
    async fn create(&self, command: CreatePolicyCommand) -> Result<Policy, CreatePolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_valid() {
        let result = ValidationResult::valid();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validation_result_invalid() {
        let errors = vec![ValidationError::new("Syntax error".to_string())];
        let result = ValidationResult::invalid(errors);
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validation_error_with_location() {
        let error = ValidationError::with_location("Missing semicolon".to_string(), 10, 25);
        assert_eq!(error.message, "Missing semicolon");
        assert_eq!(error.line, Some(10));
        assert_eq!(error.column, Some(25));
    }

    #[test]
    fn test_validation_error_without_location() {
        let error = ValidationError::new("General error".to_string());
        assert_eq!(error.message, "General error");
        assert_eq!(error.line, None);
        assert_eq!(error.column, None);
    }

    #[test]
    fn test_policy_validation_error_display() {
        let error = PolicyValidationError::ServiceError("Connection failed".to_string());
        assert_eq!(
            error.to_string(),
            "validation service error: Connection failed"
        );
    }

    #[test]
    fn test_policy_validation_timeout_display() {
        let error = PolicyValidationError::Timeout(5000);
        assert_eq!(error.to_string(), "validation timeout after 5000ms");
    }
}
