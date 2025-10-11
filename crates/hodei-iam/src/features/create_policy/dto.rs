//! Data Transfer Objects for the create_policy feature
//!
//! This module defines the command and view DTOs for creating IAM policies.
//! Following Clean Architecture, these DTOs serve as the contract between
//! the use case and external consumers.

use kernel::Hrn;
use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Command to create a new IAM policy
///
/// This command contains all the information needed to create a new policy.
/// The policy content is a Hodei policy text that will be validated before
/// persistence.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::CreatePolicyCommand;
///
/// let command = CreatePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
///     policy_content: r#"
///         permit(
///             principal,
///             action == Action::"ReadDocument",
///             resource
///         );
///     "#.to_string(),
///     description: Some("Allows reading documents".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyCommand {
    /// Unique identifier for the policy
    ///
    /// This will be used to construct the policy's HRN.
    /// Must be unique within the account.
    pub policy_id: String,

    /// Cedar policy content (policy text)
    ///
    /// This is the actual Cedar policy language text that defines
    /// the authorization rules. It will be validated before storage.
    pub policy_content: String,

    /// Optional human-readable description
    ///
    /// A brief description of what this policy does and when it should be used.
    /// This helps with policy management and audit trails.
    pub description: Option<String>,
}

impl ActionTrait for CreatePolicyCommand {
    fn name() -> &'static str {
        "CreatePolicy"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Policy".to_string()
    }
}

/// View of a created policy (DTO for responses)
///
/// This DTO represents a policy that has been successfully created
/// and persisted. It contains all the information about the policy
/// including metadata like creation timestamps.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::PolicyView;
///
/// // After creating a policy, you receive a PolicyView:
/// let view: PolicyView = use_case.execute(command).await?;
/// println!("Policy created with ID: {}", view.id);
/// println!("Content: {}", view.content);
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
    fn test_create_policy_command_serialization() {
        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let json = serde_json::to_string(&command).unwrap();
        assert!(json.contains("test-policy"));
        assert!(json.contains("permit"));
    }

    #[test]
    fn test_create_policy_command_deserialization() {
        let json = r#"{
            "policy_id": "test-policy",
            "policy_content": "permit(principal, action, resource);",
            "description": "Test policy"
        }"#;

        let command: CreatePolicyCommand = serde_json::from_str(json).unwrap();
        assert_eq!(command.policy_id, "test-policy");
        assert_eq!(command.description, Some("Test policy".to_string()));
    }

    #[test]
    fn test_create_policy_command_without_description() {
        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
        };

        assert!(command.description.is_none());
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
}
