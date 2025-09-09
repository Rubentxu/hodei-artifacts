// crates/security/src/application/ports.rs

pub mod schema_ports;

use crate::domain::authorization::{AuthorizationDecision, Principal, Action, Resource, Context};
use crate::infrastructure::errors::SecurityError;
use async_trait::async_trait;

pub use schema_ports::*;

/// Request structure for authorization evaluation
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    pub principal: Principal,
    pub action: Action,
    pub resource: Resource,
    pub context: Context,
}

/// Port (trait) for authorization services following hexagonal architecture
#[async_trait]
pub trait AuthorizationService: Send + Sync {
    /// Evaluates an authorization request and returns a decision
    async fn evaluate(&self, request: AuthorizationRequest) -> Result<AuthorizationDecision, SecurityError>;
}

impl AuthorizationRequest {
    pub fn new(principal: Principal, action: Action, resource: Resource, context: Context) -> Self {
        Self {
            principal,
            action,
            resource,
            context,
        }
    }
}