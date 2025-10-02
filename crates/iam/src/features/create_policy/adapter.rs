// crates/iam/src/features/create_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{PolicyCreator, PolicyEventPublisher, PolicyValidator};
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use async_trait::async_trait;
use cedar_policy::PolicyId;
use mongodb::{Collection, Database, bson::doc};
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
    async fn create(&self, policy: Policy) -> Result<Policy, CreatePolicyError> {
        self.collection.insert_one(&policy).await.map_err(|e| {
            CreatePolicyError::DatabaseError(format!("Failed to create policy: {}", e))
        })?;

        Ok(policy)
    }

    async fn exists(&self, id: &PolicyId) -> Result<bool, CreatePolicyError> {
        let filter = doc! { "_id": id.to_string() };
        let count = self.collection.count_documents(filter).await.map_err(|e| {
            CreatePolicyError::DatabaseError(format!("Failed to check policy existence: {}", e))
        })?;

        Ok(count > 0)
    }
}

/// Adapter that implements PolicyValidator using CedarPolicyValidator with comprehensive validation
pub struct CedarPolicyValidatorAdapter {
    validator: Arc<CedarPolicyValidator>,
}

impl CedarPolicyValidatorAdapter {
    pub fn new(validator: Arc<CedarPolicyValidator>) -> Self {
        Self { validator }
    }

    /// Perform comprehensive Cedar validation using all Cedar capabilities
    async fn validate_policy_semantics(&self, content: &str) -> Result<(), CreatePolicyError> {
        use security::ComprehensiveCedarValidator;

        let validator = ComprehensiveCedarValidator::new()
            .map_err(|e| CreatePolicyError::ValidationFailed { errors: vec![] })?;

        let result = validator
            .validate_policy_comprehensive(content)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed { errors: vec![] })?;

        if !result.is_valid {
            // Create detailed error message with all validation failures
            let mut error_parts = Vec::new();

            if !result.hrn_errors.is_empty() {
                error_parts.push(format!("HRN errors: {}", result.hrn_errors.join(", ")));
            }

            if !result.syntax_errors.is_empty() {
                error_parts.push(format!(
                    "Syntax errors: {}",
                    result.syntax_errors.join(", ")
                ));
            }

            if !result.semantic_errors.is_empty() {
                error_parts.push(format!(
                    "Semantic errors: {}",
                    result.semantic_errors.join(", ")
                ));
            }

            let error_message = if error_parts.is_empty() {
                "Policy validation failed".to_string()
            } else {
                error_parts.join("; ")
            };

            return Err(CreatePolicyError::ValidationFailed { errors: vec![] });
        }

        // Log warnings if any (but don't fail validation)
        if !result.warnings.is_empty() {
            tracing::warn!("Policy validation warnings: {}", result.warnings.join(", "));
        }

        Ok(())
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidatorAdapter {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, CreatePolicyError> {
        self.validator
            .validate_syntax(content)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed { errors: vec![] })
    }

    async fn validate_semantics(&self, content: &str) -> Result<(), CreatePolicyError> {
        self.validate_policy_semantics(content).await
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
    async fn publish_policy_created(&self, policy: &Policy) -> Result<(), CreatePolicyError> {
        self.publisher
            .publish_policy_created(policy)
            .await
            .map_err(|e| {
                CreatePolicyError::EventPublishingFailed(format!(
                    "Failed to publish policy created event: {}",
                    e
                ))
            })
    }
}
