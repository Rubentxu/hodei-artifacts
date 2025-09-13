// crates/iam/src/features/update_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::features::update_policy::ports::{PolicyUpdater, PolicyUpdateValidator, PolicyUpdateEventPublisher};
use crate::infrastructure::errors::IamError;
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use async_trait::async_trait;
use mongodb::{bson::doc, Collection, Database};
use cedar_policy::PolicyId;
use std::sync::Arc;


/// Adapter that implements PolicyUpdater using MongoDB directly
pub struct UpdatePolicyAdapter {
    collection: Collection<Policy>,
    validator: Arc<CedarPolicyValidator>,
    event_publisher: Arc<SimplePolicyEventPublisher>,
}

impl UpdatePolicyAdapter {
    pub fn new(
        database: Arc<Database>, 
        validator: Arc<CedarPolicyValidator>,
        event_publisher: Arc<SimplePolicyEventPublisher>
    ) -> Self {
        Self {
            collection: database.collection::<Policy>("policies"),
            validator,
            event_publisher,
        }
    }

    /// Perform comprehensive Cedar validation using all Cedar capabilities
    async fn validate_policy_semantics(&self, content: &str) -> Result<(), IamError> {
        use security::ComprehensiveCedarValidator;

        let validator = ComprehensiveCedarValidator::new()
            .map_err(|e| IamError::validation_error(format!("Failed to create validator: {}", e)))?;

        let result = validator.validate_policy_comprehensive(content).await
            .map_err(|e| IamError::validation_error(format!("Validation failed: {}", e)))?;

        if !result.is_valid {
            // Create detailed error message with all validation failures
            let mut error_parts = Vec::new();
            
            if !result.hrn_errors.is_empty() {
                error_parts.push(format!("HRN errors: {}", result.hrn_errors.join(", ")));
            }
            
            if !result.syntax_errors.is_empty() {
                error_parts.push(format!("Syntax errors: {}", result.syntax_errors.join(", ")));
            }
            
            if !result.semantic_errors.is_empty() {
                error_parts.push(format!("Semantic errors: {}", result.semantic_errors.join(", ")));
            }
            
            let error_message = if error_parts.is_empty() {
                "Policy validation failed".to_string()
            } else {
                error_parts.join("; ")
            };
            
            return Err(IamError::validation_error(error_message));
        }

        // Log warnings if any (but don't fail validation)
        if !result.warnings.is_empty() {
            tracing::warn!("Policy validation warnings: {}", result.warnings.join(", "));
        }

        Ok(())
    }

    /// Validate compatibility between old and new policy versions
    async fn validate_update_compatibility(&self, old_content: &str, new_content: &str) -> Result<(), IamError> {
        // First validate that both policies are individually valid
        self.validate_policy_semantics(old_content).await?;
        self.validate_policy_semantics(new_content).await?;

        // Additional compatibility checks could be added here
        // For example, checking if the new policy doesn't break existing permissions
        // or if it maintains backward compatibility

        // For now, we just ensure both are valid Cedar policies
        Ok(())
    }
}

#[async_trait]
impl PolicyUpdater for UpdatePolicyAdapter {
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "_id": id.to_string() };
        
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to get policy: {}", e)))
    }

    async fn update(&self, policy: Policy) -> Result<Policy, IamError> {
        let filter = doc! { "_id": policy.id.to_string() };

        let result = self
            .collection
            .replace_one(filter, &policy)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to update policy: {}", e)))?;

        if result.matched_count == 0 {
            return Err(IamError::PolicyNotFound(policy.id.clone()));
        }

        Ok(policy)
    }

    async fn exists(&self, id: &PolicyId) -> Result<bool, IamError> {
        let filter = doc! { "_id": id.to_string() };
        let count = self.collection
            .count_documents(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to check policy existence: {}", e)))?;
        
        Ok(count > 0)
    }
}

#[async_trait]
impl PolicyUpdateValidator for UpdatePolicyAdapter {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validator.validate_syntax(content).await
    }

    async fn validate_semantics(&self, content: &str) -> Result<(), IamError> {
        self.validate_policy_semantics(content).await
    }
}

#[async_trait]
impl PolicyUpdateEventPublisher for UpdatePolicyAdapter {
    async fn publish_policy_updated(&self, old_policy: &Policy, new_policy: &Policy) -> Result<(), IamError> {
        self.event_publisher.publish_policy_updated(old_policy, new_policy).await
    }
}

/// Implementation of the UpdatePolicySemanticValidator trait
#[async_trait]
impl crate::features::update_policy::ports::UpdatePolicySemanticValidator for UpdatePolicyAdapter {
    async fn validate_semantics(&self, policy: &str) -> Result<(), IamError> {
        self.validate_policy_semantics(policy).await
    }

    async fn validate_update_compatibility(&self, old_policy: &str, new_policy: &str) -> Result<(), IamError> {
        self.validate_update_compatibility(old_policy, new_policy).await
    }
}