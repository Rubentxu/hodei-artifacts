use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use policies::shared::domain::hrn::Hrn;

/// Request for authorization evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    /// The principal (user/service) requesting access
    pub principal: Hrn,
    /// The action being requested (e.g., "read", "write", "delete")
    pub action: String,
    /// The resource being accessed
    pub resource: Hrn,
    /// Additional context for the evaluation (optional)
    pub context: Option<AuthorizationContext>,
}

/// Additional context for authorization decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationContext {
    /// IP address of the requester
    pub source_ip: Option<String>,
    /// User agent string
    pub user_agent: Option<String>,
    /// Time of the request
    pub request_time: Option<time::OffsetDateTime>,
    /// Additional key-value context
    pub additional_context: HashMap<String, serde_json::Value>,
}

/// Response from authorization evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationResponse {
    /// The authorization decision
    pub decision: AuthorizationDecision,
    /// Policies that determined the decision
    pub determining_policies: Vec<String>,
    /// Reason for the decision
    pub reason: String,
    /// Whether the decision was explicit or implicit
    pub explicit: bool,
}

/// Authorization decision outcomes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthorizationDecision {
    /// Access is explicitly allowed
    Allow,
    /// Access is explicitly denied
    Deny,
}

/// Information about a policy that influenced the decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyImpact {
    /// ID of the policy
    pub policy_id: String,
    /// Name of the policy
    pub policy_name: String,
    /// Effect of this policy (Allow/Deny)
    pub effect: AuthorizationDecision,
    /// Whether this was a determining policy
    pub determining: bool,
}

impl Default for AuthorizationContext {
    fn default() -> Self {
        Self {
            source_ip: None,
            user_agent: None,
            request_time: Some(time::OffsetDateTime::now_utc()),
            additional_context: HashMap::new(),
        }
    }
}

impl AuthorizationRequest {
    /// Create a new authorization request
    pub fn new(principal: Hrn, action: String, resource: Hrn) -> Self {
        Self {
            principal,
            action,
            resource,
            context: None,
        }
    }

    /// Create a new authorization request with context
    pub fn with_context(
        principal: Hrn,
        action: String,
        resource: Hrn,
        context: AuthorizationContext,
    ) -> Self {
        Self {
            principal,
            action,
            resource,
            context: Some(context),
        }
    }
}

impl AuthorizationResponse {
    /// Create an allow response
    pub fn allow(policies: Vec<String>, reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Allow,
            determining_policies: policies,
            reason,
            explicit: true,
        }
    }

    /// Create a deny response
    pub fn deny(policies: Vec<String>, reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Deny,
            determining_policies: policies,
            reason,
            explicit: true,
        }
    }

    /// Create an implicit deny response (no policies matched)
    pub fn implicit_deny(reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Deny,
            determining_policies: vec![],
            reason,
            explicit: false,
        }
    }
}
