//! Ports (interfaces) for the update_policy feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (ISP) from SOLID, each port is
//! specific and minimal - containing only the operations needed by this feature.
//!
//! # Architecture
//!
//! - `PolicyValidator`: Port for validating Cedar policy syntax (re-used from create_policy)
//! - `UpdatePolicyPort`: Port for updating policies (ONLY update operation)
//!
//! # ISP Compliance
//!
//! Note that `UpdatePolicyPort` contains ONLY the `update` method, not create/delete/get/list.
//! This ensures that implementations and consumers of this port are not forced to depend
//! on operations they don't need.

use crate::features::update_policy::dto::UpdatePolicyCommand;
use crate::features::update_policy::error::UpdatePolicyError;
use async_trait::async_trait;
use policies::shared::domain::Policy;

// Re-exporting validation-related types from create_policy_new as they are identical
// and shared across features that modify policy content. This avoids duplication.
// In a more advanced scenario, these could live in a shared `validation` module.
pub use crate::features::create_policy_new::{
    PolicyValidationError, PolicyValidator, ValidationError, ValidationResult, ValidationWarning,
};

/// Port for updating IAM policies
///
/// This port defines the interface for modifying existing policies.
///
/// # Interface Segregation Principle (ISP)
///
/// **IMPORTANT**: This trait contains ONLY the `update` operation.
/// It does NOT include create, delete, get, or list operations.
///
/// This segregation ensures:
/// - Implementations only need to support updates
/// - Consumers don't depend on unused operations
/// - Each operation can evolve independently
/// - Testing is simpler (smaller interface to mock)
///
/// # Update Semantics
///
/// - **Idempotency**: Applying the same update multiple times should have the same result
/// - **Safety Checks**: Implementations should reject updates to non-existent policies
/// - **Optimistic Locking**: Implementations may use a version/etag to prevent lost updates
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct SurrealUpdatePolicyAdapter {
///     db: SurrealClient,
/// }
///
/// #[async_trait]
/// impl UpdatePolicyPort for SurrealUpdatePolicyAdapter {
///     async fn update(
///         &self,
///         command: UpdatePolicyCommand,
///     ) -> Result<Policy, UpdatePolicyError> {
///         // 1. Find policy by ID
///         // 2. Check version for optimistic locking (optional)
///         // 3. Apply updates to content and/or description
///         // 4. Persist changes and update timestamp
///         // 5. Return updated policy
///     }
/// }
/// ```
#[async_trait]
pub trait UpdatePolicyPort: Send + Sync {
    /// Update an existing policy and persist the changes
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy ID and optional new content/description
    ///
    /// # Returns
    ///
    /// The updated `Policy` entity with new metadata (timestamps, etc.)
    ///
    /// # Errors
    ///
    /// - `UpdatePolicyError::PolicyNotFound` - Policy with this ID does not exist
    /// - `UpdatePolicyError::StorageError` - Database or storage failure
    /// - `UpdatePolicyError::SystemPolicyProtected` - Cannot update a system policy
    /// - `UpdatePolicyError::VersionConflict` - Optimistic locking failure
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = UpdatePolicyCommand {
    ///     policy_id: "allow-read-docs".to_string(),
    ///     policy_content: Some("permit(...);".to_string()),
    ///     description: None,
    /// };
    ///
    /// let policy = port.update(command).await?;
    /// println!("Updated policy with HRN: {}", policy.id);
    /// ```
    async fn update(&self, command: UpdatePolicyCommand) -> Result<Policy, UpdatePolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test to verify the trait is object-safe and can be used with Arc<dyn>
    #[test]
    fn test_update_policy_port_is_object_safe() {
        fn _assert_object_safe(_port: &dyn UpdatePolicyPort) {}
    }

    #[test]
    fn test_validation_result_reexport() {
        let result = ValidationResult::valid();
        assert!(result.is_valid);
    }

    #[test]
    fn test_validation_error_reexport() {
        let error = ValidationError::new("Test error".to_string());
        assert_eq!(error.message, "Test error");
    }
}
