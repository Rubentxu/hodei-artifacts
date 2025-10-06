//! Data Transfer Objects for the update_policy feature
//!
//! This module defines the command and view DTOs for updating IAM policies.
//! Following Clean Architecture, these DTOs serve as the contract between
//! the use case and external consumers.

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Command to update an existing IAM policy
///
/// This command contains the information needed to update a policy.
/// At least one of `policy_content` or `description` must be provided.
/// The policy is identified by its ID (not the full HRN).
///
/// # Update Strategies
///
/// - **Partial Update**: Provide only the fields you want to update
/// - **Full Update**: Provide both content and description
/// - **Content Only**: Update policy logic without changing description
/// - **Description Only**: Update metadata without changing policy logic
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::UpdatePolicyCommand;
///
/// // Update both content and description
/// let command = UpdatePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
///     policy_content: Some(r#"
///         permit(
///             principal,
///             action == Action::"ReadDocument",
///             resource
///         ) when {
///             principal.department == "Engineering"
///         };
///     "#.to_string()),
///     description: Some("Updated: Only engineering can read docs".to_string()),
/// };
///
/// // Update only description
/// let command = UpdatePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
///     policy_content: None,
///     description: Some("Updated description only".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyCommand {
    /// Unique identifier for the policy to update
    ///
    /// This is the policy ID (not the full HRN).
    pub policy_id: String,

    /// Optional new Cedar policy content
    ///
    /// If provided, the policy content will be validated and updated.
    /// If None, the existing content is preserved.
    pub policy_content: Option<String>,

    /// Optional new description
    ///
    /// If provided, the description will be updated.
    /// If None, the existing description is preserved.
    /// To clear the description, pass Some("".to_string()).
    pub description: Option<String>,
}

impl UpdatePolicyCommand {
    /// Create a new update command for content only
    pub fn update_content<S1, S2>(policy_id: S1, policy_content: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        Self {
            policy_id: policy_id.into(),
            policy_content: Some(policy_content.into()),
            description: None,
        }
    }

    /// Create a new update command for description only
    pub fn update_description<S1, S2>(policy_id: S1, description: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        Self {
            policy_id: policy_id.into(),
            policy_content: None,
            description: Some(description.into()),
        }
    }

    /// Create a new update command for both content and description
    pub fn update_both<S1, S2, S3>(policy_id: S1, policy_content: S2, description: S3) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self {
            policy_id: policy_id.into(),
            policy_content: Some(policy_content.into()),
            description: Some(description.into()),
        }
    }

    /// Check if this command has any updates
    pub fn has_updates(&self) -> bool {
        self.policy_content.is_some() || self.description.is_some()
    }

    /// Check if content will be updated
    pub fn updates_content(&self) -> bool {
        self.policy_content.is_some()
    }

    /// Check if description will be updated
    pub fn updates_description(&self) -> bool {
        self.description.is_some()
    }
}

/// View of an updated policy (DTO for responses)
///
/// This DTO represents a policy that has been successfully updated.
/// It contains all the information about the policy including metadata.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::PolicyView;
///
/// // After updating a policy, you receive a PolicyView:
/// let view: PolicyView = use_case.execute(command).await?;
/// println!("Policy updated: {}", view.id);
/// println!("New content: {}", view.content);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyView {
    /// Hierarchical Resource Name (HRN) of the policy
    pub id: Hrn,

    /// The Cedar policy content as stored
    pub content: String,

    /// Optional description of the policy
    pub description: Option<String>,

    /// Timestamp when the policy was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Timestamp when the policy was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_command_content_only() {
        let command = UpdatePolicyCommand::update_content("policy1", "permit(...);");
        assert_eq!(command.policy_id, "policy1");
        assert!(command.policy_content.is_some());
        assert!(command.description.is_none());
        assert!(command.has_updates());
        assert!(command.updates_content());
        assert!(!command.updates_description());
    }

    #[test]
    fn test_update_command_description_only() {
        let command = UpdatePolicyCommand::update_description("policy1", "New description");
        assert_eq!(command.policy_id, "policy1");
        assert!(command.policy_content.is_none());
        assert!(command.description.is_some());
        assert!(command.has_updates());
        assert!(!command.updates_content());
        assert!(command.updates_description());
    }

    #[test]
    fn test_update_command_both() {
        let command = UpdatePolicyCommand::update_both("policy1", "permit(...);", "New desc");
        assert_eq!(command.policy_id, "policy1");
        assert!(command.policy_content.is_some());
        assert!(command.description.is_some());
        assert!(command.has_updates());
        assert!(command.updates_content());
        assert!(command.updates_description());
    }

    #[test]
    fn test_update_command_has_no_updates() {
        let command = UpdatePolicyCommand {
            policy_id: "policy1".to_string(),
            policy_content: None,
            description: None,
        };
        assert!(!command.has_updates());
    }

    #[test]
    fn test_update_command_serialization() {
        let command = UpdatePolicyCommand::update_content("test-policy", "permit(...);");
        let json = serde_json::to_string(&command).unwrap();
        assert!(json.contains("test-policy"));
        assert!(json.contains("permit"));
    }

    #[test]
    fn test_update_command_deserialization() {
        let json = r#"{
            "policy_id": "test-policy",
            "policy_content": "permit(...);",
            "description": "Test"
        }"#;

        let command: UpdatePolicyCommand = serde_json::from_str(json).unwrap();
        assert_eq!(command.policy_id, "test-policy");
        assert_eq!(command.policy_content, Some("permit(...);".to_string()));
        assert_eq!(command.description, Some("Test".to_string()));
    }

    #[test]
    fn test_update_command_partial_deserialization() {
        let json = r#"{
            "policy_id": "test-policy",
            "policy_content": null,
            "description": "Only description"
        }"#;

        let command: UpdatePolicyCommand = serde_json::from_str(json).unwrap();
        assert_eq!(command.policy_id, "test-policy");
        assert!(command.policy_content.is_none());
        assert!(command.description.is_some());
    }

    #[test]
    fn test_policy_view_clone() {
        let view = PolicyView {
            id: Hrn::from_string("hrn:hodei:iam::test:policy/test-policy").unwrap(),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Test".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let cloned = view.clone();
        assert_eq!(cloned.id, view.id);
        assert_eq!(cloned.content, view.content);
    }

    #[test]
    fn test_policy_view_serialization() {
        let view = PolicyView {
            id: Hrn::from_string("hrn:hodei:iam::test:policy/test-policy").unwrap(),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Test".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("test-policy"));
        assert!(json.contains("permit"));
    }
}
