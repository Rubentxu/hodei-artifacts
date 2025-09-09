// crates/iam/src/features/create_policy/ports.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use shared::hrn::PolicyId;

/// Port for policy creation operations specific to create_policy feature
#[async_trait]
pub trait PolicyCreator: Send + Sync {
    /// Create a new policy
    async fn create(&self, policy: Policy) -> Result<Policy, IamError>;
    
    /// Check if a policy exists (to prevent duplicates)
    async fn exists(&self, id: &PolicyId) -> Result<bool, IamError>;
}

/// Port for policy validation specific to create_policy feature
#[async_trait]
pub trait PolicyValidator: Send + Sync {
    /// Validate Cedar policy syntax
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError>;
    
    /// Validate Cedar policy semantics against schema
    async fn validate_semantics(&self, content: &str) -> Result<(), IamError>;
}

/// Port for semantic policy validation specific to create_policy feature
#[async_trait]
pub trait CreatePolicySemanticValidator: Send + Sync {
    /// Validate policy semantics against Cedar schema
    async fn validate_semantics(&self, policy: &str) -> Result<(), IamError>;
    
    /// Validate policy against schema and return detailed validation result
    async fn validate_against_schema(&self, policy: &str) -> Result<ValidationResult, IamError>;
}

/// Port for publishing events specific to create_policy feature
#[async_trait]
pub trait PolicyEventPublisher: Send + Sync {
    /// Publish policy created event
    async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError>;
}