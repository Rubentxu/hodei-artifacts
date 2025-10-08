//! Engine Types - Public API Types for Authorization Engine
//!
//! This module defines the public types used by the authorization engine.
//! All types are agnostic and do not expose Cedar implementation details.

use kernel::HodeiEntity;
use std::collections::HashMap;

// ============================================================================
// Core Types
// ============================================================================

/// Authorization Decision Result
///
/// Represents the result of an authorization evaluation.
/// This is a simple boolean decision with optional diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationDecision {
    /// Whether the action is allowed or denied
    decision: Decision,
    /// Reason for the decision (for debugging)
    reason: String,
    /// IDs of policies that determined the decision
    determining_policies: Vec<String>,
}

impl AuthorizationDecision {
    /// Create an allow decision
    pub fn allow() -> Self {
        Self {
            decision: Decision::Allow,
            reason: "Access granted".to_string(),
            determining_policies: Vec::new(),
        }
    }

    /// Create an allow decision with a custom reason
    #[allow(dead_code)]
    pub fn allow_with_reason(reason: String) -> Self {
        Self {
            decision: Decision::Allow,
            reason,
            determining_policies: Vec::new(),
        }
    }

    /// Create a deny decision
    pub fn deny() -> Self {
        Self {
            decision: Decision::Deny,
            reason: "Access denied".to_string(),
            determining_policies: Vec::new(),
        }
    }

    /// Create a deny decision with a custom reason
    #[allow(dead_code)]
    pub fn deny_with_reason(reason: String) -> Self {
        Self {
            decision: Decision::Deny,
            reason,
            determining_policies: Vec::new(),
        }
    }

    /// Add determining policies to the decision
    #[allow(dead_code)]
    pub fn with_policies(mut self, policies: Vec<String>) -> Self {
        self.determining_policies = policies;
        self
    }

    /// Check if the decision is allow
    pub fn is_allowed(&self) -> bool {
        matches!(self.decision, Decision::Allow)
    }

    /// Get the decision
    #[allow(dead_code)]
    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// Get the reason for the decision
    #[allow(dead_code)]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Get the determining policies
    #[allow(dead_code)]
    pub fn determining_policies(&self) -> &[String] {
        &self.determining_policies
    }
}

/// Simple decision enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Action is allowed
    Allow,
    /// Action is denied
    Deny,
}

/// Authorization Engine Error
///
/// Represents all possible errors that can occur during authorization.
#[derive(thiserror::Error, Debug, Clone)]
#[allow(dead_code)]
pub enum EngineError {
    /// Policy has invalid syntax or structure
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),

    /// Failed to translate agnostic types to Cedar types
    #[error("Translation error: {0}")]
    TranslationError(String),

    /// Policy evaluation failed
    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Required entity is not registered
    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    /// Internal error (should not happen)
    #[error("Internal error: {0}")]
    InternalError(String),
}

// ============================================================================
// Request Types
// ============================================================================

/// Authorization Request
///
/// Represents a request to authorize an action on a resource by a principal.
/// All types are references to kernel agnostic types.
pub struct EngineRequest<'a> {
    /// Principal performing the action
    pub principal: &'a dyn HodeiEntity,
    /// Action being requested
    pub action: &'a str,
    /// Resource the action is being performed on
    pub resource: &'a dyn HodeiEntity,
    /// Additional context for policy evaluation
    pub context: HashMap<String, serde_json::Value>,
}

impl<'a> EngineRequest<'a> {
    /// Create a new authorization request
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

    /// Add context to the request
    pub fn with_context(mut self, context: HashMap<String, serde_json::Value>) -> Self {
        self.context = context;
        self
    }

    /// Get the principal's HRN
    #[allow(dead_code)]
    pub fn principal_hrn(&self) -> &kernel::Hrn {
        self.principal.hrn()
    }

    /// Get the resource's HRN
    #[allow(dead_code)]
    pub fn resource_hrn(&self) -> &kernel::Hrn {
        self.resource.hrn()
    }
}

// ============================================================================
// Policy Types
// ============================================================================

/// Policy Document
///
/// Represents a policy with its ID and content.
/// The content is stored as a Cedar DSL string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct PolicyDocument {
    /// Policy identifier
    pub id: String,
    /// Policy content in Cedar DSL
    pub content: String,
}

#[allow(dead_code)]
impl PolicyDocument {
    /// Create a new policy document
    pub fn new(id: String, content: String) -> Self {
        Self { id, content }
    }

    /// Get the policy ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the policy content
    pub fn content(&self) -> &str {
        &self.content
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{
        AttributeName, AttributeType, AttributeValue, ResourceTypeName, ServiceName,
    };
    use kernel::{HodeiEntity, HodeiEntityType, Hrn};
    use std::collections::HashMap;

    #[derive(Debug)]
    struct TestEntity {
        hrn: Hrn,
    }

    impl HodeiEntityType for TestEntity {
        fn service_name() -> ServiceName {
            ServiceName::new("test").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Entity").unwrap()
        }
    }

    impl HodeiEntity for TestEntity {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            HashMap::new()
        }
    }

    #[test]
    fn authorization_decision_allow() {
        let decision = AuthorizationDecision::allow();
        assert!(decision.is_allowed());
        assert_eq!(decision.decision(), Decision::Allow);
    }

    #[test]
    fn authorization_decision_deny() {
        let decision = AuthorizationDecision::deny();
        assert!(!decision.is_allowed());
        assert_eq!(decision.decision(), Decision::Deny);
    }

    #[test]
    fn authorization_decision_with_reason() {
        let decision = AuthorizationDecision::allow_with_reason("Custom reason".to_string());
        assert!(decision.is_allowed());
        assert_eq!(decision.reason(), "Custom reason");
    }

    #[test]
    fn authorization_decision_with_policies() {
        let decision = AuthorizationDecision::allow()
            .with_policies(vec!["policy1".to_string(), "policy2".to_string()]);
        assert!(decision.is_allowed());
        assert_eq!(decision.determining_policies().len(), 2);
    }

    #[test]
    fn engine_request_creation() {
        let entity = TestEntity {
            hrn: Hrn::new(
                "aws".to_string(),
                "test".to_string(),
                "123".to_string(),
                "Entity".to_string(),
                "test".to_string(),
            ),
        };

        let request = EngineRequest::new(&entity, "read", &entity);
        assert_eq!(request.action, "read");
        assert_eq!(request.context.len(), 0);
    }

    #[test]
    fn engine_request_with_context() {
        let entity = TestEntity {
            hrn: Hrn::new(
                "aws".to_string(),
                "test".to_string(),
                "123".to_string(),
                "Entity".to_string(),
                "test".to_string(),
            ),
        };

        let mut context = HashMap::new();
        context.insert("ip".to_string(), serde_json::json!("192.168.1.1"));

        let request = EngineRequest::new(&entity, "read", &entity).with_context(context);

        assert_eq!(
            request.context.get("ip"),
            Some(&serde_json::json!("192.168.1.1"))
        );
    }

    #[test]
    fn policy_document_creation() {
        let policy = PolicyDocument::new(
            "policy1".to_string(),
            "permit(principal, action, resource);".to_string(),
        );

        assert_eq!(policy.id(), "policy1");
        assert_eq!(policy.content(), "permit(principal, action, resource);");
    }
}
