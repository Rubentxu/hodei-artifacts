// crates/iam/src/features/create_policy/dto.rs

use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use serde::{Deserialize, Serialize};

/// Command to create a new policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyCommand {
    /// Name of the policy
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Cedar policy content
    pub content: String,
    /// Optional tags
    pub tags: Option<Vec<String>>,
    /// User creating the policy
    pub created_by: String,
}

/// Response after creating a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyResponse {
    /// The created policy
    pub policy: Policy,
    /// Success message
    pub message: String,
}

impl CreatePolicyCommand {
    /// Create a new create policy command
    pub fn new(name: String, content: String, created_by: String) -> Self {
        Self {
            name,
            description: None,
            content,
            tags: None,
            created_by,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Set tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Validate the command
    pub fn validate(&self) -> Result<(), IamError> {
        // Validate name
        if self.name.trim().is_empty() {
            return Err(IamError::InvalidInput("Name cannot be empty".to_string()));
        }
        if self.name.len() > 255 {
            return Err(IamError::InvalidInput(
                "Name cannot exceed 255 characters".to_string(),
            ));
        }

        // Validate content
        if self.content.trim().is_empty() {
            return Err(IamError::InvalidInput("Content cannot be empty".to_string()));
        }

        // Validate created_by
        if self.created_by.trim().is_empty() {
            return Err(IamError::InvalidInput(
                "Created by field cannot be empty".to_string(),
            ));
        }

        // Validate tags if provided
        if let Some(tags) = &self.tags {
            if tags.len() > 10 {
                return Err(IamError::InvalidInput(
                    "Cannot have more than 10 tags".to_string(),
                ));
            }

            for tag in tags {
                if tag.trim().is_empty() {
                    return Err(IamError::InvalidInput("Tags cannot be empty".to_string()));
                }
                if tag.len() > 50 {
                    return Err(IamError::InvalidInput(
                        "Tag cannot exceed 50 characters".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

impl CreatePolicyResponse {
    /// Create a new create policy response
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            message: "Policy created successfully".to_string(),
        }
    }
}#[cfg(test)
]
mod tests {
    use super::*;

    #[test]
    fn test_create_policy_command_new() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        assert_eq!(command.name, "Test Policy");
        assert_eq!(command.content, "permit(principal, action, resource);");
        assert_eq!(command.created_by, "user_123");
        assert_eq!(command.description, None);
        assert_eq!(command.tags, None);
    }

    #[test]
    fn test_create_policy_command_with_description() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_description("A test policy".to_string());

        assert_eq!(command.description, Some("A test policy".to_string()));
    }

    #[test]
    fn test_create_policy_command_with_tags() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_tags(vec!["engineering".to_string(), "test".to_string()]);

        assert_eq!(command.tags, Some(vec!["engineering".to_string(), "test".to_string()]));
    }

    #[test]
    fn test_create_policy_command_validate_success() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        assert!(command.validate().is_ok());
    }

    #[test]
    fn test_create_policy_command_validate_empty_name() {
        let command = CreatePolicyCommand::new(
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Name cannot be empty"));
    }

    #[test]
    fn test_create_policy_command_validate_long_name() {
        let long_name = "a".repeat(256);
        let command = CreatePolicyCommand::new(
            long_name,
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Name cannot exceed 255 characters"));
    }

    #[test]
    fn test_create_policy_command_validate_empty_content() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "".to_string(),
            "user_123".to_string(),
        );

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Content cannot be empty"));
    }

    #[test]
    fn test_create_policy_command_validate_empty_created_by() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "".to_string(),
        );

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Created by field cannot be empty"));
    }

    #[test]
    fn test_create_policy_command_validate_too_many_tags() {
        let tags = (0..11).map(|i| format!("tag{}", i)).collect();
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_tags(tags);

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot have more than 10 tags"));
    }

    #[test]
    fn test_create_policy_command_validate_empty_tag() {
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_tags(vec!["".to_string()]);

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Tags cannot be empty"));
    }

    #[test]
    fn test_create_policy_command_validate_long_tag() {
        let long_tag = "a".repeat(51);
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_tags(vec![long_tag]);

        let result = command.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Tag cannot exceed 50 characters"));
    }
}