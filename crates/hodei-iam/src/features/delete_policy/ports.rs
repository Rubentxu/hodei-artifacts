//! Ports (interfaces) for the delete_policy feature
//!
//! This module defines the port (trait interface) that the use case depends on.
//! Following the Interface Segregation Principle (ISP) from SOLID, this port is
//! specific and minimal - containing only the operations needed by this feature.
//!
//! # Architecture
//!
//! - `DeletePolicyPort`: Port for deleting policies (ONLY delete operation)
//!
//! # ISP Compliance
//!
//! Note that `DeletePolicyPort` contains ONLY the `delete` method, not create/update/get/list.
//! This ensures that implementations and consumers of this port are not forced to depend
//! on operations they don't need.
//!
//! This is part of the refactored segregated architecture where each CRUD operation
//! has its own dedicated port instead of a monolithic "PolicyRepository" trait.

use crate::features::delete_policy::error::DeletePolicyError;
use async_trait::async_trait;

/// Port for deleting IAM policies
///
/// This port defines the interface for removing policies from the system.
///
/// # Interface Segregation Principle (ISP)
///
/// **IMPORTANT**: This trait contains ONLY the `delete` operation.
/// It does NOT include create, update, get, or list operations.
///
/// This segregation ensures:
/// - Implementations only need to support deletion
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
/// # Policy Deletion Semantics
///
/// - Idempotent: Deleting a non-existent policy may return an error or succeed (implementation choice)
/// - Safety checks: Implementations may reject deletion if the policy is in use
/// - Soft delete: Implementations may use soft deletion (marking as deleted) vs hard deletion
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct SurrealDeletePolicyAdapter {
///     db: SurrealClient,
/// }
///
/// #[async_trait]
/// impl DeletePolicyPort for SurrealDeletePolicyAdapter {
///     async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
///         // 1. Check if policy exists
///         // 2. Check if policy is in use (optional safety check)
///         // 3. Delete from SurrealDB
///         // 4. Return success or appropriate error
///     }
/// }
/// ```
#[async_trait]
pub trait DeletePolicyPort: Send + Sync {
    /// Delete a policy by its ID
    ///
    /// # Arguments
    ///
    /// * `policy_id` - The unique identifier of the policy to delete (not the full HRN)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the policy was successfully deleted.
    ///
    /// # Errors
    ///
    /// - `DeletePolicyError::PolicyNotFound` - Policy with this ID does not exist
    /// - `DeletePolicyError::PolicyInUse` - Policy is attached to users/groups and cannot be deleted
    /// - `DeletePolicyError::SystemPolicyProtected` - Policy is a system policy and cannot be deleted
    /// - `DeletePolicyError::StorageError` - Database or storage failure
    /// - `DeletePolicyError::InvalidPolicyId` - Policy ID format is invalid
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// port.delete("allow-read-docs").await?;
    /// println!("Policy deleted successfully");
    /// ```
    ///
    /// # Implementation Notes
    ///
    /// Implementations should:
    /// - Validate that the policy exists before deletion
    /// - Optionally check if the policy is in use and reject deletion
    /// - Handle system/protected policies appropriately
    /// - Use transactions to ensure atomicity
    /// - Log deletion events for audit trails
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::delete_policy::error::DeletePolicyError;

    // Simple test to verify the trait is object-safe and can be used with Arc<dyn>
    #[test]
    fn test_delete_policy_port_is_object_safe() {
        // This test ensures the trait can be used as a trait object
        fn _assert_object_safe(_port: &dyn DeletePolicyPort) {}
    }

    #[test]
    fn test_delete_policy_error_types() {
        let error = DeletePolicyError::PolicyNotFound("test".to_string());
        assert!(error.to_string().contains("not found"));

        let error = DeletePolicyError::PolicyInUse("attached to users".to_string());
        assert!(error.to_string().contains("in use"));
    }
}
