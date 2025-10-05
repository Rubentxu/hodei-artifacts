//! Public types for the Authorization Engine
//!
//! This module defines the agnostic API types that external crates use
//! to interact with the authorization engine. NO Cedar types are exposed.

use kernel::domain::AttributeValue;
use kernel::{HodeiEntity, Hrn};
use std::collections::HashMap;
use thiserror::Error;
use cedar_policy::entities_errors::EntitiesError;

// ============================================================================
// Request Types (Agnostic API)
// ============================================================================

/// Request for authorization evaluation using agnostic types
///
/// This struct is completely Cedar-agnostic. External crates create this
/// using only types from the kernel crate.
///
/// # Examples
///
/// ```rust,ignore
/// use policies::engine::{EngineRequest, AuthorizationEngine};
/// use kernel::HodeiEntity;
///
/// let request = EngineRequest {
///     principal: &user,  // &dyn HodeiEntity
///     action: "ReadDocument",
///     resource: &document,  // &dyn HodeiEntity
///     context: HashMap::new(),
/// };
///
/// let allowed = engine.is_authorized(request)?;
/// ```
pub struct EngineRequest<'a> {
    /// The principal performing the action (e.g., User)
    pub principal: &'a dyn HodeiEntity,

    /// The action being performed (e.g., "ReadDocument", "CreateUser")
    pub action: &'a str,

    /// The resource being accessed (e.g., Document, Bucket)
    pub resource: &'a dyn HodeiEntity,

    /// Additional context attributes for evaluation
    pub context: HashMap<String, AttributeValue>,
}

impl<'a> EngineRequest<'a> {
    /// Create a new engine request with the given principal, action, and resource
    pub fn new(
        principal: &'a dyn HodeiEntity,
        action: &'a str,
        resource: &'a dyn HodeiEntity,
    ) -> Self {
        Self {
            principal,
            action,
            resource,
            context: HashMap::new(),
        }
    }

    /// Create a new engine request with context attributes
    pub fn with_context(
        principal: &'a dyn HodeiEntity,
        action: &'a str,
        resource: &'a dyn HodeiEntity,
        context: HashMap<String, AttributeValue>,
    ) -> Self {
        Self {
            principal,
            action,
            resource,
            context,
        }
    }

    /// Add a context attribute
    pub fn add_context(&mut self, key: String, value: AttributeValue) {
        self.context.insert(key, value);
    }

    /// Get principal HRN
    pub fn principal_hrn(&self) -> &Hrn {
        self.principal.hrn()
    }

    /// Get resource HRN
    pub fn resource_hrn(&self) -> &Hrn {
        self.resource.hrn()
    }
}

// ============================================================================
// Response Types (Agnostic API)
// ============================================================================

/// Result of an authorization evaluation
///
/// This is a simple boolean decision with optional diagnostic information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationDecision {
    /// Whether the action is allowed
    pub allowed: bool,

    /// The decision in enum form for pattern matching
    pub decision: Decision,

    /// Optional reason for the decision (for debugging/auditing)
    pub reason: Option<String>,

    /// Policies that contributed to the decision (policy IDs)
    pub determining_policies: Vec<String>,
}

impl AuthorizationDecision {
    /// Create an "allow" decision
    pub fn allow() -> Self {
        Self {
            allowed: true,
            decision: Decision::Allow,
            reason: None,
            determining_policies: Vec::new(),
        }
    }

    /// Create a "deny" decision
    pub fn deny() -> Self {
        Self {
            allowed: false,
            decision: Decision::Deny,
            reason: None,
            determining_policies: Vec::new(),
        }
    }

    /// Create an "allow" decision with reason
    pub fn allow_with_reason(reason: String) -> Self {
        Self {
            allowed: true,
            decision: Decision::Allow,
            reason: Some(reason),
            determining_policies: Vec::new(),
        }
    }

    /// Create a "deny" decision with reason
    pub fn deny_with_reason(reason: String) -> Self {
        Self {
            allowed: false,
            decision: Decision::Deny,
            reason: Some(reason),
            determining_policies: Vec::new(),
        }
    }

    /// Add determining policies
    pub fn with_policies(mut self, policies: Vec<String>) -> Self {
        self.determining_policies = policies;
        self
    }

    /// Check if the decision is "allow"
    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    /// Check if the decision is "deny"
    pub fn is_denied(&self) -> bool {
        !self.allowed
    }
}

/// Simple decision enum for pattern matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The action is allowed
    Allow,
    /// The action is denied
    Deny,
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during authorization engine operations
#[derive(Debug, Error)]
pub enum EngineError {
    /// Translation from agnostic types to Cedar failed
    #[error("Translation error: {0}")]
    TranslationError(String),

    /// Cedar policy evaluation failed
    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Invalid policy syntax
    #[error("Invalid policy syntax: {0}")]
    InvalidPolicy(String),

    /// Entity not found in the entity store
    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    /// Schema validation error
    #[error("Schema error: {0}")]
    SchemaError(String),

    /// Internal Cedar error
    #[error("Cedar internal error: {0}")]
    CedarError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

// Conversion from translator errors
impl From<crate::shared::infrastructure::translator::TranslatorError> for EngineError {
    fn from(err: crate::shared::infrastructure::translator::TranslatorError) -> Self {
        EngineError::TranslationError(err.to_string())
    }
}

// Conversion from Cedar entities errors
impl From<EntitiesError> for EngineError {
    fn from(err: EntitiesError) -> Self {
        EngineError::EvaluationFailed(format!("Entities error: {}", err))
    }
}

// ============================================================================
// Schema Types
// ============================================================================

/// Schema configuration for the authorization engine
///
/// The schema defines the entity types, attributes, and actions that the
/// engine understands. It must be configured before evaluating policies.
#[derive(Debug, Clone)]
pub struct SchemaConfig {
    /// Cedar schema in JSON or DSL format
    pub schema_source: SchemaSource,
}

/// Source of the Cedar schema
#[derive(Debug, Clone)]
pub enum SchemaSource {
    /// Schema defined in Cedar DSL format
    CedarDsl(String),

    /// Schema defined in JSON format
    Json(String),

    /// Schema will be built programmatically
    Dynamic,
}

// ============================================================================
// Policy Types
// ============================================================================

/// A policy document in Cedar DSL format
///
/// Policies are stored as strings (Cedar DSL) and parsed/validated by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDocument {
    /// Unique identifier for the policy
    pub id: String,

    /// The policy content in Cedar DSL format
    pub content: String,

    /// Optional description
    pub description: Option<String>,
}

impl PolicyDocument {
    /// Create a new policy document
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            description: None,
        }
    }

    /// Create a policy with description
    pub fn with_description(
        id: impl Into<String>,
        content: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            description: Some(description.into()),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_decision_allow() {
        let decision = AuthorizationDecision::allow();
        assert!(decision.is_allowed());
        assert!(!decision.is_denied());
        assert_eq!(decision.decision, Decision::Allow);
    }

    #[test]
    fn authorization_decision_deny() {
        let decision = AuthorizationDecision::deny();
        assert!(!decision.is_allowed());
        assert!(decision.is_denied());
        assert_eq!(decision.decision, Decision::Deny);
    }

    #[test]
    fn authorization_decision_with_reason() {
        let decision = AuthorizationDecision::allow_with_reason("Policy ABC allows".to_string());
        assert!(decision.is_allowed());
        assert_eq!(decision.reason, Some("Policy ABC allows".to_string()));
    }

    #[test]
    fn authorization_decision_with_policies() {
        let decision = AuthorizationDecision::allow()
            .with_policies(vec!["policy1".to_string(), "policy2".to_string()]);
        assert_eq!(decision.determining_policies.len(), 2);
    }

    #[test]
    fn policy_document_creation() {
        let policy = PolicyDocument::new("test-policy", "permit(principal, action, resource);");
        assert_eq!(policy.id, "test-policy");
        assert_eq!(policy.content, "permit(principal, action, resource);");
        assert_eq!(policy.description, None);
    }

    #[test]
    fn policy_document_with_description() {
        let policy = PolicyDocument::with_description(
            "test-policy",
            "permit(principal, action, resource);",
            "Test policy",
        );
        assert_eq!(policy.description, Some("Test policy".to_string()));
    }

    #[test]
    fn error_display() {
        let err = EngineError::EvaluationFailed("test error".to_string());
        assert!(err.to_string().contains("Evaluation failed"));
        assert!(err.to_string().contains("test error"));
    }
}
