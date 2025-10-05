//! # Port Interfaces for Create Policy Feature
//!
//! This module defines the abstraction layer (ports) that the `CreatePolicyUseCase` depends on.
//! Following the Dependency Inversion Principle from SOLID, the use case depends on these
//! trait abstractions rather than concrete implementations.
//!
//! ## Design Principles
//!
//! - **Interface Segregation**: Each port defines only the methods needed by this specific feature
//! - **Async by Default**: All ports use async traits for non-blocking operations
//! - **Domain-Centric**: Ports work with domain types (`Policy`, `PolicyId`), not primitives
//! - **Error Transparency**: Errors are feature-specific, not generic
//!
//! ## Port Descriptions
//!
//! - **`PolicyIdGenerator`**: Generates unique identifiers for policies
//! - **`PolicyValidator`**: Validates policy syntax and semantics (Cedar-aware internally)
//! - **`PolicyPersister`**: Saves policies to persistent storage
//!
//! These abstractions allow different implementations to be injected based on context:
//! - Production: Real Cedar validator, UUID generator, database persister
//! - Testing: Mocks with configurable behavior
//! - Development: In-memory storage, simplified validation

use crate::features::create_policy::dto::PolicyContent;
use crate::features::create_policy::error::CreatePolicyError;
use crate::shared::domain::policy::{Policy, PolicyId};
use async_trait::async_trait;

/// Port for generating unique policy identifiers.
///
/// This abstraction allows different ID generation strategies to be used without
/// changing the use case logic. Implementations might include:
/// - UUID v4 generation
/// - Sequential numeric IDs
/// - Custom format (e.g., "policy-{timestamp}-{random}")
/// - Distributed ID generation (e.g., Snowflake IDs)
///
/// # Example Implementation
///
/// ```rust,ignore
/// use uuid::Uuid;
/// use async_trait::async_trait;
///
/// pub struct UuidPolicyIdGenerator;
///
/// #[async_trait]
/// impl PolicyIdGenerator for UuidPolicyIdGenerator {
///     async fn generate(&self) -> Result<PolicyId, CreatePolicyError> {
///         Ok(PolicyId::new(Uuid::new_v4().to_string()))
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyIdGenerator: Send + Sync {
    /// Generates a new, unique policy ID.
    ///
    /// Implementations should ensure uniqueness with high probability, though the
    /// use case will also check for conflicts before persisting.
    ///
    /// # Returns
    ///
    /// - `Ok(PolicyId)` containing the generated ID
    /// - `Err(CreatePolicyError)` if ID generation fails
    ///
    /// # Errors
    ///
    /// May return `CreatePolicyError::Internal` if the generation process fails
    /// (e.g., random number generator failure, external service unavailable).
    async fn generate(&self) -> Result<PolicyId, CreatePolicyError>;
}

/// Port for validating policy content.
///
/// This abstraction encapsulates all Cedar-specific validation logic, keeping the
/// use case free from direct dependencies on the Cedar framework. This is a critical
/// architectural boundary that enables:
/// - Testing without Cedar dependencies
/// - Potential migration to different policy engines
/// - Custom validation logic per deployment
///
/// Implementations typically validate:
/// - **Syntax**: Is it valid Cedar policy language?
/// - **Semantics**: Are referenced types/attributes defined in the schema?
/// - **Best Practices**: Does it follow organizational policy guidelines?
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
/// use cedar_policy::Policy as CedarPolicy;
///
/// pub struct CedarPolicyValidator;
///
/// #[async_trait]
/// impl PolicyValidator for CedarPolicyValidator {
///     async fn validate(&self, content: &PolicyContent) -> Result<(), CreatePolicyError> {
///         CedarPolicy::parse(None, content.as_ref())
///             .map_err(|e| CreatePolicyError::ValidationError(format!("Invalid syntax: {}", e)))?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyValidator: Send + Sync {
    /// Validates the provided policy content.
    ///
    /// This method should perform comprehensive validation including syntax and
    /// semantics. It should NOT perform business rule validation (that's the
    /// use case's responsibility).
    ///
    /// # Arguments
    ///
    /// * `content` - The policy content to validate
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the policy is valid
    /// - `Err(CreatePolicyError::ValidationError)` if validation fails
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::ValidationError` - The policy has syntax or semantic errors
    /// - `CreatePolicyError::Internal` - An unexpected error occurred during validation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let content = PolicyContent::new("permit(principal, action, resource);")?;
    /// validator.validate(&content).await?;
    /// // If we reach here, the policy is valid
    /// ```
    async fn validate(&self, content: &PolicyContent) -> Result<(), CreatePolicyError>;
}

/// Port for persisting policies to storage.
///
/// This abstraction separates the use case from storage implementation details,
/// enabling different persistence strategies:
/// - In-memory (for testing/development)
/// - Relational database (PostgreSQL, MySQL)
/// - NoSQL database (SurrealDB, MongoDB)
/// - File system
/// - Distributed cache with write-through
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
/// use std::sync::Arc;
/// use dashmap::DashMap;
///
/// pub struct InMemoryPolicyPersister {
///     store: Arc<DashMap<PolicyId, Policy>>,
/// }
///
/// #[async_trait]
/// impl PolicyPersister for InMemoryPolicyPersister {
///     async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError> {
///         if self.store.contains_key(policy.id()) {
///             return Err(CreatePolicyError::Conflict(policy.id().to_string()));
///         }
///         self.store.insert(policy.id().clone(), policy.clone());
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyPersister: Send + Sync {
    /// Saves a policy to persistent storage.
    ///
    /// This method should:
    /// - Store the complete policy entity (ID, content, metadata)
    /// - Ensure atomicity (all-or-nothing)
    /// - Check for ID conflicts and return appropriate errors
    /// - Set appropriate timestamps (created_at)
    ///
    /// # Arguments
    ///
    /// * `policy` - The policy entity to persist
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the policy was successfully saved
    /// - `Err(CreatePolicyError)` if the save operation fails
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::Conflict` - A policy with the same ID already exists
    /// - `CreatePolicyError::Internal` - Storage operation failed (database error, etc.)
    ///
    /// # Idempotency
    ///
    /// This operation is NOT idempotent. Calling it twice with the same policy
    /// should result in a `Conflict` error on the second call.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let policy = Policy::new(id, content, metadata);
    /// persister.save(&policy).await?;
    /// // Policy is now stored and retrievable
    /// ```
    async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests verify that the traits are correctly defined and can be used
    // in a generic context. They don't test implementations (that's done in adapter tests).

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn policy_id_generator_is_send_sync() {
        // This ensures the trait can be used in async contexts
        assert_send_sync::<Box<dyn PolicyIdGenerator>>();
    }

    #[test]
    fn policy_validator_is_send_sync() {
        assert_send_sync::<Box<dyn PolicyValidator>>();
    }

    #[test]
    fn policy_persister_is_send_sync() {
        assert_send_sync::<Box<dyn PolicyPersister>>();
    }
}
