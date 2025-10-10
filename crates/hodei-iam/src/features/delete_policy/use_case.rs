//! Use case for deleting IAM policies
//!
//! This module implements the business logic for deleting IAM policies.
//! Following Clean Architecture and Vertical Slice Architecture (VSA) principles,
//! this use case is self-contained and depends only on abstract ports.
//!
//! # Flow
//!
//! 1. Receive `DeletePolicyCommand` from the caller
//! 2. Validate policy ID (not empty)
//! 3. Optionally check if policy is in use (future enhancement)
//! 4. Delete the policy through `DeletePolicyPort`
//! 5. Return success or appropriate error
//!
//! # Dependencies
//!
//! - `DeletePolicyPort`: Abstract port for policy deletion (ISP - only delete)
//! - `DeletePolicyUseCasePort`: Port for executing the use case

use crate::features::delete_policy::dto::DeletePolicyCommand;
use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::ports::{DeletePolicyPort, DeletePolicyUseCasePort};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// Use case for deleting IAM policies
///
/// This use case orchestrates the policy deletion process:
/// 1. Validates the policy ID
/// 2. Deletes the policy through the port
/// 3. Returns success or appropriate error
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::{DeletePolicyUseCase, DeletePolicyCommand};
/// use std::sync::Arc;
///
/// let deleter = Arc::new(SurrealPolicyAdapter::new(db));
/// let use_case = DeletePolicyUseCase::new(deleter);
///
/// let command = DeletePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
/// };
///
/// match use_case.execute(command).await {
///     Ok(()) => println!("Policy deleted successfully"),
///     Err(e) => eprintln!("Deletion failed: {}", e),
/// }
/// ```
pub struct DeletePolicyUseCase {
    /// Port for deleting policies (only delete operation)
    policy_port: Arc<dyn DeletePolicyPort>,
}

impl DeletePolicyUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_port` - Implementation of `DeletePolicyPort` for deletion
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let use_case = DeletePolicyUseCase::new(Arc::new(policy_port));
    /// ```
    pub fn new(policy_port: Arc<dyn DeletePolicyPort>) -> Self {
        Self { policy_port }
    }

    /// Execute the delete policy use case
    ///
    /// This is the main entry point for deleting an IAM policy.
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing the policy ID to delete
    ///
    /// # Returns
    ///
    /// On success, returns `Ok(())` indicating the policy was deleted.
    ///
    /// # Errors
    ///
    /// - `DeletePolicyError::InvalidPolicyId` - Policy ID is invalid or empty
    /// - `DeletePolicyError::PolicyNotFound` - Policy does not exist
    /// - `DeletePolicyError::PolicyInUse` - Policy is attached to users/groups
    /// - `DeletePolicyError::SystemPolicyProtected` - Cannot delete system policy
    /// - `DeletePolicyError::StorageError` - Database or storage failure
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = DeletePolicyCommand {
    ///     policy_id: "my-policy".to_string(),
    /// };
    ///
    /// use_case.execute(command).await?;
    /// println!("Policy deleted");
    /// ```
    #[instrument(skip(self, command), fields(policy_id = %command.policy_id))]
    pub async fn execute(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        let mut command = command;

        let normalized_policy_id = command.policy_id.trim();
        info!("Deleting policy with id: {}", normalized_policy_id);

        // Validate input
        if normalized_policy_id.is_empty() {
            warn!("Policy deletion failed: empty policy ID");
            return Err(DeletePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        // Validate policy ID format (basic alphanumeric + hyphens + underscores)
        if !is_valid_policy_id(normalized_policy_id) {
            warn!(
                "Policy deletion failed: invalid policy ID format: {}",
                normalized_policy_id
            );
            return Err(DeletePolicyError::InvalidPolicyId(format!(
                "Policy ID '{}' contains invalid characters. Only alphanumeric, hyphens, and underscores are allowed.",
                normalized_policy_id
            )));
        }

        command.policy_id = normalized_policy_id.to_string();

        // Delete policy through port
        info!("Deleting policy from storage");
        self.policy_port
            .delete(&command.policy_id)
            .await
            .map_err(|e| {
                warn!("Policy deletion failed: {}", e);
                e
            })?;

        info!("Policy deleted successfully: {}", command.policy_id);
        Ok(())
    }
}

// Implement DeletePolicyPort trait for the use case to enable trait object usage
#[async_trait]
impl DeletePolicyPort for DeletePolicyUseCase {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        let command = DeletePolicyCommand {
            policy_id: policy_id.to_string(),
        };
        self.execute(command).await
    }
}

// Implement DeletePolicyUseCasePort trait for the use case
#[async_trait]
impl DeletePolicyUseCasePort for DeletePolicyUseCase {
    async fn execute(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        self.execute(command).await
    }
}

/// Validate policy ID format
///
/// Policy IDs must:
/// - Not be empty
/// - Contain only alphanumeric characters, hyphens, and underscores
/// - Start with an alphanumeric character
/// - Not exceed 128 characters
fn is_valid_policy_id(policy_id: &str) -> bool {
    if policy_id.is_empty() || policy_id.len() > 128 {
        return false;
    }

    // Must start with alphanumeric
    let first_char = policy_id.chars().next().unwrap();
    if !first_char.is_alphanumeric() {
        return false;
    }

    // All characters must be alphanumeric, hyphen, or underscore
    policy_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_policy_id() {
        // Valid IDs
        assert!(is_valid_policy_id("policy1"));
        assert!(is_valid_policy_id("my-policy"));
        assert!(is_valid_policy_id("my_policy"));
        assert!(is_valid_policy_id("Policy123"));
        assert!(is_valid_policy_id("a"));

        // Invalid IDs
        assert!(!is_valid_policy_id("")); // empty
        assert!(!is_valid_policy_id("-starts-with-hyphen"));
        assert!(!is_valid_policy_id("_starts_with_underscore"));
        assert!(!is_valid_policy_id("has spaces"));
        assert!(!is_valid_policy_id("has@special"));
        assert!(!is_valid_policy_id("has/slash"));
        assert!(!is_valid_policy_id(&"a".repeat(129))); // too long
    }

    #[test]
    fn test_valid_policy_id_edge_cases() {
        assert!(is_valid_policy_id("a1"));
        assert!(is_valid_policy_id("1a")); // starts with number is ok
        assert!(is_valid_policy_id("policy-with-many-hyphens-is-ok"));
        assert!(is_valid_policy_id("policy_with_many_underscores_is_ok"));
        assert!(is_valid_policy_id(&"a".repeat(128))); // max length
    }
}
