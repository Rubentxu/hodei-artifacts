//! Ports (trait definitions) for the playground_evaluate feature
//!
//! This module defines the public interfaces that the PlaygroundEvaluateUseCase
//! depends on. These traits enable dependency inversion and testability.
//!
//! Following ISP (Interface Segregation Principle), each trait is minimal
//! and focused on a single responsibility.

use async_trait::async_trait;
use cedar_policy::Schema;

use super::dto::{AttributeValue, Decision, DeterminingPolicy, PlaygroundAuthorizationRequest};
use super::error::PlaygroundEvaluateError;

/// Port for loading Cedar schemas (inline or from storage)
///
/// This trait abstracts schema loading, allowing the use case to work
/// with both inline schemas and stored schema versions.
#[async_trait]
pub trait SchemaLoaderPort: Send + Sync {
    /// Load a schema from inline JSON or a stored version
    ///
    /// # Arguments
    ///
    /// * `inline_schema` - Optional inline schema as JSON string
    /// * `schema_version` - Optional reference to stored schema version
    ///
    /// # Returns
    ///
    /// A parsed Cedar Schema ready for validation and evaluation
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Both inline and version are provided
    /// - Neither inline nor version are provided
    /// - Schema parsing fails
    /// - Stored schema is not found
    async fn load_schema(
        &self,
        inline_schema: Option<String>,
        schema_version: Option<String>,
    ) -> Result<Schema, PlaygroundEvaluateError>;
}

/// Port for validating Cedar policies against a schema
///
/// This trait provides policy validation services, ensuring that
/// inline policies conform to the loaded schema.
#[async_trait]
pub trait PolicyValidatorPort: Send + Sync {
    /// Validate a list of policy texts against a schema
    ///
    /// # Arguments
    ///
    /// * `policy_texts` - List of Cedar policy strings to validate
    /// * `schema` - The Cedar schema to validate against
    ///
    /// # Returns
    ///
    /// A list of validation errors (empty if all policies are valid)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Policy parsing fails
    /// - Schema validation fails
    /// - Internal validation error occurs
    async fn validate_policies(
        &self,
        policy_texts: &[String],
        schema: &Schema,
    ) -> Result<Vec<String>, PlaygroundEvaluateError>;
}

/// Port for evaluating authorization requests against policies
///
/// This trait handles the core Cedar authorization logic, evaluating
/// requests against a set of policies and returning decisions.
#[async_trait]
pub trait PolicyEvaluatorPort: Send + Sync {
    /// Evaluate an authorization request against inline policies
    ///
    /// # Arguments
    ///
    /// * `request` - The authorization request (principal, action, resource, context)
    /// * `policy_texts` - List of Cedar policy strings to evaluate
    /// * `schema` - The Cedar schema for entity validation
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The authorization decision (Allow or Deny)
    /// - List of policies that determined the decision
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Policy parsing fails
    /// - Request translation fails
    /// - Evaluation engine fails
    async fn evaluate(
        &self,
        request: &PlaygroundAuthorizationRequest,
        policy_texts: &[String],
        schema: &Schema,
    ) -> Result<(Decision, Vec<DeterminingPolicy>), PlaygroundEvaluateError>;
}

/// Port for converting context attributes to Cedar format
///
/// This trait handles the conversion of playground context attributes
/// to Cedar's internal representation.
pub trait ContextConverterPort: Send + Sync {
    /// Convert playground attribute values to Cedar context
    ///
    /// # Arguments
    ///
    /// * `attributes` - Map of attribute names to values
    ///
    /// # Returns
    ///
    /// A Cedar-compatible context map
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Attribute type is invalid
    /// - Conversion fails
    fn convert_context(
        &self,
        attributes: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<
        std::collections::HashMap<String, cedar_policy::RestrictedExpression>,
        PlaygroundEvaluateError,
    >;
}

/// Port trait for playground policy evaluation
///
/// This trait defines the contract for the playground evaluation use case.
/// It represents the use case's public interface.
#[async_trait]
pub trait PlaygroundEvaluatePort: Send + Sync {
    /// Execute a playground evaluation
    ///
    /// This method evaluates authorization requests against inline policies
    /// and schemas in a playground environment without requiring persistence.
    ///
    /// # Arguments
    ///
    /// * `command` - The evaluation command containing policies, schema, and request
    ///
    /// # Returns
    ///
    /// The evaluation result with decision, diagnostics, and determining policies
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema loading fails
    /// - Policy validation fails
    /// - Policy evaluation fails
    /// - Context conversion fails
    async fn evaluate(
        &self,
        command: super::dto::PlaygroundEvaluateCommand,
    ) -> Result<super::dto::PlaygroundEvaluateResult, PlaygroundEvaluateError>;
}
