//! Ports (interfaces) for the create_policy feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (ISP) from SOLID, each port is
//! specific and minimal - containing only the operations needed by this feature.
//!
//! # Architecture
//!
//! - `PolicyValidator`: Port for validating Cedar policy syntax
//! - `CreatePolicyPort`: Port for persisting policies (ONLY create operation)
//!
//! # ISP Compliance
//!
//! Note that `CreatePolicyPort` contains ONLY the `create` method, not update/delete/get/list.
//! This ensures that implementations and consumers of this port are not forced to depend
//! on operations they don't need.

use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::create_policy::error::CreatePolicyError;
use async_trait::async_trait;
// use hodei_policies::features::validate_policy::ValidatePolicyPort; // Temporarily disabled - unused
use kernel::domain::policy::HodeiPolicy;

/// Port for validating IAM policy content
///
/// This port directly uses the ValidatePolicyPort from hodei-policies crate,
/// following the principle that use cases should expose their own trait interface.
///
/// # Purpose
///
/// Validates that a Cedar policy text is syntactically and semantically correct
/// before it is persisted. This prevents storing invalid policies that would
/// cause runtime errors during authorization.
///
/// # Architecture Decision
///
/// Instead of creating a duplicate trait in hodei-iam, we use the standard
/// trait exposed by hodei-policies. This ensures:
/// - Single source of truth for the validation interface
/// - No duplication of contracts
/// - Consistent behavior across all consumers
/// - True separation of implementation from interface
pub use hodei_policies::features::validate_policy::ValidatePolicyPort as PolicyValidator;

/// Re-export validation result types from hodei-policies
pub use hodei_policies::features::validate_policy::dto::ValidationResult;
pub use hodei_policies::features::validate_policy::error::ValidatePolicyError as PolicyValidationError;

/// Port for creating IAM policies
///
/// This port defines the interface for persisting newly created policies.
///
/// # Interface Segregation Principle (ISP)
///
/// **IMPORTANT**: This trait contains ONLY the `create` operation.
/// It does NOT include update, delete, get, or list operations.
///
/// This segregation ensures:
/// - Implementations only need to support creation
/// - Consumers don't depend on unused operations
/// - Each operation can evolve independently
/// - Testing is simpler (smaller interface to mock)
///
/// # Why Segregated?
///
/// In the original monolithic `PolicyPersister` trait, all CRUD operations
/// were mixed together. This violated ISP because:
/// - A feature that only needs DELETE was forced to depend on CREATE, UPDATE, GET, LIST
/// - Mocks had to implement all 5 methods even if only testing 1
/// - Changes to one operation affected all consumers
///
/// By segregating into separate traits (`CreatePolicyPort`, `DeletePolicyPort`, etc.),
/// each feature can depend only on what it needs.
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct SurrealCreatePolicyAdapter {
///     db: SurrealClient,
/// }
///
/// #[async_trait]
/// impl CreatePolicyPort for SurrealCreatePolicyAdapter {
///     async fn create(
///         &self,
///         command: CreatePolicyCommand,
///     ) -> Result<Policy, CreatePolicyError> {
///         // Insert policy into SurrealDB
///         // Return created policy with metadata
///     }
/// }
/// ```
#[async_trait]
pub trait CreatePolicyPort: Send + Sync {
    /// Create a new policy and persist it
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy details (id, content, description)
    ///
    /// # Returns
    ///
    /// The created `Policy` entity with generated metadata (timestamps, HRN, etc.)
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::StorageError` - Database or storage failure
    /// - `CreatePolicyError::PolicyAlreadyExists` - Policy with this ID already exists
    /// - `CreatePolicyError::InvalidPolicyId` - Policy ID format is invalid
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = CreatePolicyCommand {
    ///     policy_id: "allow-read-docs".to_string(),
    ///     policy_content: "permit(...);".to_string(),
    ///     description: Some("Allow document reading".to_string()),
    /// };
    ///
    /// let policy = port.create(command).await?;
    /// println!("Created policy with HRN: {}", policy.id);
    /// ```
    async fn create(&self, command: CreatePolicyCommand) -> Result<HodeiPolicy, CreatePolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests removed - validation types are now re-exported from hodei-policies
}
