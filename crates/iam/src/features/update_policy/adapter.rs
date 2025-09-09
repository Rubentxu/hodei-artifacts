// crates/iam/src/features/update_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::features::update_policy::ports::{PolicyUpdater, PolicyUpdateValidator, PolicyUpdateEventPublisher};
use crate::infrastructure::errors::IamError;
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use async_trait::async_trait;
use mongodb::{bson::doc, Collection, Database};
use shared::hrn::PolicyId;
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
}

#[async_trait]
impl PolicyUpdater for UpdatePolicyAdapter {
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "_id": id.0.to_string() };
        
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to get policy: {}", e)))
    }

    async fn update(&self, policy: Policy) -> Result<Policy, IamError> {
        let filter = doc! { "_id": policy.id.0.to_string() };

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
        let filter = doc! { "_id": id.0.to_string() };
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
}

#[async_trait]
impl PolicyUpdateEventPublisher for UpdatePolicyAdapter {
    async fn publish_policy_updated(&self, old_policy: &Policy, new_policy: &Policy) -> Result<(), IamError> {
        self.event_publisher.publish_policy_updated(old_policy, new_policy).await
    }
}