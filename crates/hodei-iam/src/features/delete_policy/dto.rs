//! Data Transfer Objects for the delete_policy feature
//!
//! This module defines the command DTO for deleting IAM policies.
//! Following Clean Architecture, these DTOs serve as the contract between
//! the use case and external consumers.

use serde::{Deserialize, Serialize};

/// Command to delete an existing IAM policy
///
/// This command contains the information needed to identify and delete a policy.
/// The policy is identified by its ID (not the full HRN).
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::DeletePolicyCommand;
///
/// let command = DeletePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletePolicyCommand {
    /// Unique identifier for the policy to delete
    ///
    /// This is the policy ID (not the full HRN).
    /// The use case will construct the HRN internally if needed.
    pub policy_id: String,
}

impl DeletePolicyCommand {
    /// Create a new delete policy command
    ///
    /// # Arguments
    ///
    /// * `policy_id` - The unique identifier of the policy to delete
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = DeletePolicyCommand::new("my-policy");
    /// ```
    pub fn new<S: Into<String>>(policy_id: S) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_policy_command_new() {
        let command = DeletePolicyCommand::new("test-policy");
        assert_eq!(command.policy_id, "test-policy");
    }

    #[test]
    fn test_delete_policy_command_serialization() {
        let command = DeletePolicyCommand {
            policy_id: "test-policy".to_string(),
        };

        let json = serde_json::to_string(&command).unwrap();
        assert!(json.contains("test-policy"));
    }

    #[test]
    fn test_delete_policy_command_deserialization() {
        let json = r#"{
            "policy_id": "test-policy"
        }"#;

        let command: DeletePolicyCommand = serde_json::from_str(json).unwrap();
        assert_eq!(command.policy_id, "test-policy");
    }

    #[test]
    fn test_delete_policy_command_clone() {
        let command = DeletePolicyCommand::new("test-policy");
        let cloned = command.clone();
        assert_eq!(cloned.policy_id, command.policy_id);
    }

    #[test]
    fn test_delete_policy_command_equality() {
        let cmd1 = DeletePolicyCommand::new("test-policy");
        let cmd2 = DeletePolicyCommand::new("test-policy");
        let cmd3 = DeletePolicyCommand::new("other-policy");

        assert_eq!(cmd1, cmd2);
        assert_ne!(cmd1, cmd3);
    }
}
