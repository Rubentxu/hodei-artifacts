// crates/security/src/features/evaluate_policy/handler.rs

use crate::application::ports::{AuthorizationService, AuthorizationRequest};
use crate::infrastructure::errors::SecurityError;
use crate::features::evaluate_policy::command::{EvaluatePolicyCommand, EvaluatePolicyResponse};
use std::sync::Arc;

/// Handler for policy evaluation following VSA patterns
pub struct EvaluatePolicyHandler {
    authorization_service: Arc<dyn AuthorizationService>,
}

impl EvaluatePolicyHandler {
    pub fn new(authorization_service: Arc<dyn AuthorizationService>) -> Self {
        Self { authorization_service }
    }

    pub async fn handle(&self, command: EvaluatePolicyCommand) -> Result<EvaluatePolicyResponse, SecurityError> {
        // Validate the command
        self.validate_command(&command)?;
        
        // Convert command to authorization request
        let request = AuthorizationRequest::new(
            command.principal,
            command.action,
            command.resource,
            command.context,
        );
        
        // Delegate to authorization service
        let decision = self.authorization_service.evaluate(request).await?;
        
        Ok(EvaluatePolicyResponse::new(decision))
    }

    fn validate_command(&self, command: &EvaluatePolicyCommand) -> Result<(), SecurityError> {
        if command.principal.id.is_empty() {
            return Err(SecurityError::InvalidRequest("Principal ID cannot be empty".to_string()));
        }
        
        if command.principal.entity_type.is_empty() {
            return Err(SecurityError::InvalidRequest("Principal entity type cannot be empty".to_string()));
        }
        
        if command.action.name.is_empty() {
            return Err(SecurityError::InvalidRequest("Action name cannot be empty".to_string()));
        }
        
        if command.resource.id.is_empty() {
            return Err(SecurityError::InvalidRequest("Resource ID cannot be empty".to_string()));
        }
        
        if command.resource.entity_type.is_empty() {
            return Err(SecurityError::InvalidRequest("Resource entity type cannot be empty".to_string()));
        }
        
        Ok(())
    }
}