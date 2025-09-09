// crates/iam/src/features/create_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::features::create_policy::ports::{PolicyCreator, PolicyValidator, PolicyEventPublisher};
use crate::infrastructure::errors::IamError;
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use async_trait::async_trait;
use mongodb::{bson::doc, Collection, Database};
use shared::hrn::PolicyId;
use std::sync::Arc;

/// Adapter that implements PolicyCreator using MongoDB directly
pub struct MongoPolicyCreatorAdapter {
    collection: Collection<Policy>,
}

impl MongoPolicyCreatorAdapter {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            collection: database.collection::<Policy>("policies"),
        }
    }
}

#[async_trait]
impl PolicyCreator for MongoPolicyCreatorAdapter {
    async fn create(&self, policy: Policy) -> Result<Policy, IamError> {
        self.collection
            .insert_one(&policy)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to create policy: {}", e)))?;
        
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

/// Adapter that implements PolicyValidator using CedarPolicyValidator
pub struct CedarPolicyValidatorAdapter {
    validator: Arc<CedarPolicyValidator>,
}

impl CedarPolicyValidatorAdapter {
    pub fn new(validator: Arc<CedarPolicyValidator>) -> Self {
        Self { validator }
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidatorAdapter {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validator.validate_syntax(content).await
    }
}

/// Adapter that implements PolicyEventPublisher using SimplePolicyEventPublisher
pub struct SimplePolicyEventPublisherAdapter {
    publisher: Arc<SimplePolicyEventPublisher>,
}

impl SimplePolicyEventPublisherAdapter {
    pub fn new(publisher: Arc<SimplePolicyEventPublisher>) -> Self {
        Self { publisher }
    }
}

#[async_trait]
impl PolicyEventPublisher for SimplePolicyEventPublisherAdapter {
    async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError> {
        self.publisher.publish_policy_created(policy).await
    }
}