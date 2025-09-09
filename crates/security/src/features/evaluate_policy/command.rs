// crates/security/src/features/evaluate_policy/command.rs

use crate::domain::authorization::{Principal, Action, Resource, Context, AuthorizationDecision};

/// Command for evaluating a policy authorization request
#[derive(Debug, Clone)]
pub struct EvaluatePolicyCommand {
    pub principal: Principal,
    pub action: Action,
    pub resource: Resource,
    pub context: Context,
}

/// Response from policy evaluation
#[derive(Debug, Clone)]
pub struct EvaluatePolicyResponse {
    pub decision: AuthorizationDecision,
}

impl EvaluatePolicyCommand {
    pub fn new(principal: Principal, action: Action, resource: Resource, context: Context) -> Self {
        Self {
            principal,
            action,
            resource,
            context,
        }
    }
}

impl EvaluatePolicyResponse {
    pub fn new(decision: AuthorizationDecision) -> Self {
        Self { decision }
    }
}