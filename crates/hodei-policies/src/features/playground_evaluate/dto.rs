//! Data Transfer Objects for the playground_evaluate feature
//!
//! This module defines the input and output DTOs for ad-hoc policy evaluation
//! in the playground environment, where policies and schemas can be tested
//! without persistence.

use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Command to evaluate policies in the playground
///
/// This command allows evaluation of ad-hoc policies against a request,
/// optionally with an inline schema or a reference to a stored schema version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundEvaluateCommand {
    /// Optional inline Cedar schema (JSON format)
    /// If None, must provide schema_version
    pub inline_schema: Option<String>,

    /// Optional reference to a stored schema version
    /// If None, must provide inline_schema
    pub schema_version: Option<String>,

    /// Inline Cedar policies to evaluate (policy text)
    /// Each string is a complete Cedar policy
    pub inline_policies: Vec<String>,

    /// The authorization request to evaluate
    pub request: PlaygroundAuthorizationRequest,
}

impl PlaygroundEvaluateCommand {
    /// Crea un comando usando un esquema en línea (JSON)
    pub fn new_with_inline_schema(
        inline_schema: String,
        inline_policies: Vec<String>,
        request: PlaygroundAuthorizationRequest,
    ) -> Self {
        Self {
            inline_schema: Some(inline_schema),
            schema_version: None,
            inline_policies,
            request,
        }
    }

    /// Crea un comando usando una versión de esquema almacenada
    pub fn new_with_schema_version(
        schema_version: String,
        inline_policies: Vec<String>,
        request: PlaygroundAuthorizationRequest,
    ) -> Self {
        Self {
            inline_schema: None,
            schema_version: Some(schema_version),
            inline_policies,
            request,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.inline_schema.is_none() && self.schema_version.is_none() {
            return Err("Debe proporcionar inline_schema o schema_version (no ambos None)".to_string());
        }
        if self.inline_schema.is_some() && self.schema_version.is_some() {
            return Err("No puede proporcionar inline_schema y schema_version al mismo tiempo".to_string());
        }
        if self.inline_policies.is_empty() {
            return Err("Debe proporcionar al menos una política en inline_policies".to_string());
        }
        Ok(())
    }
}

impl ActionTrait for PlaygroundEvaluateCommand {
    fn name() -> &'static str {
        "PlaygroundEvaluate"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("policies").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Policies::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Policies::Policy".to_string()
    }
}

/// Authorization request for playground evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundAuthorizationRequest {
    /// The principal (user/service) making the request
    pub principal: Hrn,

    /// The action being requested
    pub action: Hrn,

    /// The resource being accessed
    pub resource: Hrn,

    /// Optional context attributes for the request
    #[serde(default)]
    pub context: HashMap<String, AttributeValue>,
}

impl PlaygroundAuthorizationRequest {
    /// Create a new authorization request
    pub fn new(principal: Hrn, action: Hrn, resource: Hrn) -> Self {
        Self {
            principal,
            action,
            resource,
            context: HashMap::new(),
        }
    }

    /// Add a context attribute
    pub fn with_context(mut self, key: String, value: AttributeValue) -> Self {
        self.context.insert(key, value);
        self
    }
}

/// Attribute value for context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AttributeValue {
    /// String value
    String(String),
    /// Integer value
    Long(i64),
    /// Boolean value
    Bool(bool),
    /// Entity reference (HRN)
    EntityRef(Hrn),
    /// Set of values
    Set(Vec<AttributeValue>),
    /// Record (map) of values
    Record(HashMap<String, AttributeValue>),
}

/// Result of playground policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundEvaluateResult {
    /// The authorization decision (Allow/Deny)
    pub decision: Decision,

    /// Policies that contributed to the decision
    pub determining_policies: Vec<DeterminingPolicy>,

    /// Diagnostic information about the evaluation
    pub diagnostics: EvaluationDiagnostics,

    /// Errors encountered during evaluation (if any)
    pub errors: Vec<String>,
}

impl PlaygroundEvaluateResult {
    /// Create a new evaluation result
    pub fn new(
        decision: Decision,
        determining_policies: Vec<DeterminingPolicy>,
        diagnostics: EvaluationDiagnostics,
    ) -> Self {
        Self {
            decision,
            determining_policies,
            diagnostics,
            errors: vec![],
        }
    }

    /// Add an error to the result
    pub fn with_error(mut self, error: String) -> Self {
        self.errors.push(error);
        self
    }

    /// Add multiple errors to the result
    pub fn with_errors(mut self, errors: Vec<String>) -> Self {
        self.errors.extend(errors);
        self
    }
}

/// Authorization decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Decision {
    /// Access is allowed
    Allow,
    /// Access is denied
    Deny,
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::Allow => write!(f, "ALLOW"),
            Decision::Deny => write!(f, "DENY"),
        }
    }
}

/// Information about a policy that contributed to the decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminingPolicy {
    /// The policy ID or inline index
    pub policy_id: String,

    /// The effect of the policy (permit or forbid)
    pub effect: PolicyEffect,

    /// The policy text (for inline policies)
    pub policy_text: Option<String>,
}

impl DeterminingPolicy {
    /// Create a new determining policy
    pub fn new(policy_id: String, effect: PolicyEffect) -> Self {
        Self {
            policy_id,
            effect,
            policy_text: None,
        }
    }

    /// Add the policy text
    pub fn with_text(mut self, text: String) -> Self {
        self.policy_text = Some(text);
        self
    }
}

/// Policy effect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyEffect {
    /// Permit effect
    Permit,
    /// Forbid effect
    Forbid,
}

impl std::fmt::Display for PolicyEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyEffect::Permit => write!(f, "permit"),
            PolicyEffect::Forbid => write!(f, "forbid"),
        }
    }
}

/// Diagnostic information about the evaluation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvaluationDiagnostics {
    /// Total number of policies evaluated
    pub total_policies: usize,

    /// Number of policies that matched
    pub matched_policies: usize,

    /// Whether schema validation was performed
    pub schema_validated: bool,

    /// Validation errors (if any)
    pub validation_errors: Vec<String>,

    /// Warnings (if any)
    pub warnings: Vec<String>,
}

impl EvaluationDiagnostics {
    /// Create new diagnostics
    pub fn new(total_policies: usize, matched_policies: usize) -> Self {
        Self {
            total_policies,
            matched_policies,
            schema_validated: false,
            validation_errors: vec![],
            warnings: vec![],
        }
    }

    /// Mark schema as validated
    pub fn with_schema_validation(mut self) -> Self {
        self.schema_validated = true;
        self
    }

    /// Add a validation error
    pub fn add_validation_error(&mut self, error: String) {
        self.validation_errors.push(error);
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_validation_requires_schema() {
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );

        let cmd = PlaygroundEvaluateCommand {
            inline_schema: None,
            schema_version: None,
            inline_policies: vec!["permit(principal, action, resource);".to_string()],
            request,
        };

        assert!(cmd.validate().is_err());
    }

    #[test]
    fn test_command_validation_requires_policies() {
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );

        let cmd = PlaygroundEvaluateCommand {
            inline_schema: Some("{}".to_string()),
            schema_version: None,
            inline_policies: vec![],
            request,
        };

        assert!(cmd.validate().is_err());
    }

    #[test]
    fn test_command_validation_cannot_have_both_schemas() {
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );

        let cmd = PlaygroundEvaluateCommand {
            inline_schema: Some("{}".to_string()),
            schema_version: Some("v1".to_string()),
            inline_policies: vec!["permit(principal, action, resource);".to_string()],
            request,
        };

        assert!(cmd.validate().is_err());
    }

    #[test]
    fn test_command_validation_success_with_inline_schema() {
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );

        let cmd = PlaygroundEvaluateCommand::new_with_inline_schema(
            "{}".to_string(),
            vec!["permit(principal, action, resource);".to_string()],
            request,
        );

        assert!(cmd.validate().is_ok());
    }

    #[test]
    fn test_decision_display() {
        assert_eq!(Decision::Allow.to_string(), "ALLOW");
        assert_eq!(Decision::Deny.to_string(), "DENY");
    }

    #[test]
    fn test_authorization_request_with_context() {
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        )
        .with_context(
            "ip".to_string(),
            AttributeValue::String("192.168.1.1".to_string()),
        );

        assert_eq!(request.context.len(), 1);
        assert!(request.context.contains_key("ip"));
    }
}
