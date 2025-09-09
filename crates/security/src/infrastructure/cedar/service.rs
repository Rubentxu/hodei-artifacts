// crates/security/src/infrastructure/cedar/service.rs

use cedar_policy::{Authorizer, PolicySet, Entities, Request, Response, Decision, EntityUid, Context as CedarContext, RestrictedExpression};
use crate::application::ports::{AuthorizationService, AuthorizationRequest};
use crate::domain::authorization::{AuthorizationDecision, AttributeValue};
use crate::infrastructure::errors::SecurityError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::str::FromStr;

/// Cedar-based implementation of the AuthorizationService
pub struct CedarAuthorizationService {
    authorizer: Authorizer,
    policy_set: PolicySet,
}

impl CedarAuthorizationService {
    pub fn new() -> Result<Self, SecurityError> {
        let authorizer = Authorizer::new();
        let policy_set = Self::create_default_policy_set()?;
        
        Ok(Self {
            authorizer,
            policy_set,
        })
    }

    fn create_default_policy_set() -> Result<PolicySet, SecurityError> {
        // Create basic hardcoded policies for initial testing
        let policy_src = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"test-artifact"
            );
            
            permit(
                principal == User::"admin",
                action,
                resource
            );
        "#;
        
        PolicySet::from_str(policy_src)
            .map_err(|e| SecurityError::PolicyParseError(e.to_string()))
    }

    fn convert_to_cedar_request(&self, request: &AuthorizationRequest) -> Result<Request, SecurityError> {
        // Convert principal - ensure proper quoting for entity IDs
        let principal_uid = EntityUid::from_str(&format!("{}::\"{}\"", request.principal.entity_type, request.principal.id))
            .map_err(|e| SecurityError::ConversionError(format!("Invalid principal ID: {}", e)))?;
        
        // Convert action
        let action_uid = EntityUid::from_str(&format!("Action::\"{}\"", request.action.name))
            .map_err(|e| SecurityError::ConversionError(format!("Invalid action ID: {}", e)))?;
        
        // Convert resource
        let resource_uid = EntityUid::from_str(&format!("{}::\"{}\"", request.resource.entity_type, request.resource.id))
            .map_err(|e| SecurityError::ConversionError(format!("Invalid resource ID: {}", e)))?;
        
        // Convert context attributes
        let context = self.convert_context(&request.context.attributes)?;
        
        Request::new(
            principal_uid,
            action_uid,
            resource_uid,
            context,
            None, // schema for now
        ).map_err(|e| SecurityError::CedarEngineError(e.to_string()))
    }

    fn convert_context(&self, attributes: &HashMap<String, AttributeValue>) -> Result<CedarContext, SecurityError> {
        let mut context_map = HashMap::new();
        
        for (key, value) in attributes {
            let expr = self.convert_attribute_value(value)?;
            context_map.insert(key.clone(), expr);
        }
        
        CedarContext::from_pairs(context_map)
            .map_err(|e| SecurityError::ConversionError(format!("Failed to create context: {}", e)))
    }

    fn convert_attribute_value(&self, value: &AttributeValue) -> Result<RestrictedExpression, SecurityError> {
        match value {
            AttributeValue::String(s) => {
                RestrictedExpression::from_str(&format!("\"{}\"", s))
                    .map_err(|e| SecurityError::ConversionError(format!("Failed to convert string: {}", e)))
            },
            AttributeValue::Long(n) => {
                RestrictedExpression::from_str(&n.to_string())
                    .map_err(|e| SecurityError::ConversionError(format!("Failed to convert long: {}", e)))
            },
            AttributeValue::Boolean(b) => {
                RestrictedExpression::from_str(&b.to_string())
                    .map_err(|e| SecurityError::ConversionError(format!("Failed to convert boolean: {}", e)))
            },
            _ => Err(SecurityError::ConversionError("Complex attribute types not yet supported".to_string()))
        }
    }

    fn convert_decision(&self, response: Response) -> AuthorizationDecision {
        match response.decision() {
            Decision::Allow => AuthorizationDecision::Allow,
            Decision::Deny => AuthorizationDecision::Deny,
        }
    }
}

#[async_trait]
impl AuthorizationService for CedarAuthorizationService {
    async fn evaluate(&self, request: AuthorizationRequest) -> Result<AuthorizationDecision, SecurityError> {
        let cedar_request = self.convert_to_cedar_request(&request)?;
        let entities = Entities::empty(); // Start with empty entities for basic testing
        
        let response = self.authorizer.is_authorized(&cedar_request, &self.policy_set, &entities);
        
        Ok(self.convert_decision(response))
    }
}

impl Default for CedarAuthorizationService {
    fn default() -> Self {
        Self::new().expect("Failed to create default CedarAuthorizationService")
    }
}