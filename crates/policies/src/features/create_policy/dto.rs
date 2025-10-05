//! # Data Transfer Objects for Create Policy Feature
//!
//! This module defines the DTOs (Data Transfer Objects) used in the `create_policy` feature.
//! Following Clean Architecture and VSA principles, these DTOs serve as the contract between
//! the use case and external layers (API, CLI, etc.).
//!
//! ## Design Principles
//!
//! - **Validation at Construction**: DTOs validate their invariants in constructors
//! - **Value Objects**: Use newtype wrappers for semantic type safety
//! - **Explicit Errors**: Return domain-specific errors, not generic strings
//! - **Self-Documenting**: Rich documentation explains purpose and constraints

use crate::features::create_policy::error::CreatePolicyError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A validated policy content wrapper.
///
/// This is a value object that ensures policy content is never empty or whitespace-only.
/// It's used throughout the feature to provide compile-time guarantees about content validity.
///
/// # Example
///
/// ```rust,ignore
/// use policies::features::create_policy::dto::PolicyContent;
///
/// // Valid content
/// let content = PolicyContent::new("permit(principal, action, resource);".to_string())?;
///
/// // Invalid content (empty) - returns error
/// let invalid = PolicyContent::new("".to_string());
/// assert!(invalid.is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyContent(String);

impl PolicyContent {
    /// Creates a new `PolicyContent` from a string.
    ///
    /// # Arguments
    ///
    /// * `content` - The policy content as a string
    ///
    /// # Returns
    ///
    /// - `Ok(PolicyContent)` if the content is valid (non-empty, non-whitespace)
    /// - `Err(CreatePolicyError::InvalidInput)` if the content is empty or whitespace-only
    ///
    /// # Errors
    ///
    /// Returns `CreatePolicyError::InvalidInput` if:
    /// - The content is empty
    /// - The content contains only whitespace characters
    pub fn new(content: String) -> Result<Self, CreatePolicyError> {
        if content.trim().is_empty() {
            return Err(CreatePolicyError::InvalidInput(
                "Policy content cannot be empty or whitespace-only".to_string(),
            ));
        }
        Ok(Self(content))
    }

    /// Returns the inner string content.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for PolicyContent {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PolicyContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Command to create a new policy.
///
/// This is the primary input DTO for the `CreatePolicyUseCase`. It encapsulates all
/// the data needed to create a policy, with validation performed at construction time
/// to ensure invariants are maintained.
///
/// # Validation Rules
///
/// - **Content**: Must not be empty or whitespace-only (enforced by `PolicyContent`)
/// - **Description**: If provided, must not be empty or whitespace-only
/// - **Tags**: If provided, must not be empty and must not contain duplicates
///
/// # Example
///
/// ```rust,ignore
/// use policies::features::create_policy::dto::CreatePolicyCommand;
///
/// let command = CreatePolicyCommand::new(
///     "permit(principal, action, resource);".to_string(),
///     Some("Allow all access".to_string()),
///     Some(vec!["production".to_string(), "critical".to_string()])
/// )?;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePolicyCommand {
    /// The validated policy content
    content: PolicyContent,

    /// Optional human-readable description
    description: Option<String>,

    /// Optional tags for categorization and filtering
    tags: Vec<String>,
}

impl CreatePolicyCommand {
    /// Creates a new `CreatePolicyCommand` with validation.
    ///
    /// # Arguments
    ///
    /// * `content` - The policy content as a string
    /// * `description` - Optional description (must not be empty if provided)
    /// * `tags` - Optional tags (must not be empty or contain duplicates if provided)
    ///
    /// # Returns
    ///
    /// - `Ok(CreatePolicyCommand)` if all inputs are valid
    /// - `Err(CreatePolicyError::InvalidInput)` if any validation fails
    ///
    /// # Errors
    ///
    /// Returns `CreatePolicyError::InvalidInput` if:
    /// - Content is empty or whitespace-only
    /// - Description is provided but empty or whitespace-only
    /// - Tags list is provided but empty
    /// - Tags list contains duplicate values
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Valid command
    /// let command = CreatePolicyCommand::new(
    ///     "permit(principal, action, resource);".to_string(),
    ///     Some("My policy".to_string()),
    ///     Some(vec!["tag1".to_string(), "tag2".to_string()])
    /// )?;
    ///
    /// // Invalid - empty content
    /// let invalid = CreatePolicyCommand::new("".to_string(), None, None);
    /// assert!(invalid.is_err());
    ///
    /// // Invalid - duplicate tags
    /// let invalid = CreatePolicyCommand::new(
    ///     "permit(principal, action, resource);".to_string(),
    ///     None,
    ///     Some(vec!["tag1".to_string(), "tag1".to_string()])
    /// );
    /// assert!(invalid.is_err());
    /// ```
    pub fn new(
        content: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<Self, CreatePolicyError> {
        // Validate content (will return error if empty)
        let policy_content = PolicyContent::new(content)?;

        // Validate description if provided
        let validated_description = if let Some(desc) = description {
            if desc.trim().is_empty() {
                return Err(CreatePolicyError::InvalidInput(
                    "Description cannot be empty or whitespace-only if provided".to_string(),
                ));
            }
            Some(desc)
        } else {
            None
        };

        // Validate tags if provided
        let validated_tags = if let Some(tag_list) = tags {
            if tag_list.is_empty() {
                return Err(CreatePolicyError::InvalidInput(
                    "Tags list cannot be empty if provided".to_string(),
                ));
            }

            // Check for duplicates
            let unique_tags: HashSet<_> = tag_list.iter().collect();
            if unique_tags.len() != tag_list.len() {
                return Err(CreatePolicyError::InvalidInput(
                    "Tags list cannot contain duplicate values".to_string(),
                ));
            }

            tag_list
        } else {
            Vec::new()
        };

        Ok(Self {
            content: policy_content,
            description: validated_description,
            tags: validated_tags,
        })
    }

    /// Returns a reference to the policy content.
    pub fn content(&self) -> &PolicyContent {
        &self.content
    }

    /// Returns the description, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns a reference to the tags.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Consumes the command and returns its components.
    pub fn into_parts(self) -> (PolicyContent, Option<String>, Vec<String>) {
        (self.content, self.description, self.tags)
    }
}

/// Response DTO after successful policy creation.
///
/// This DTO is returned by the use case to communicate the result of a successful
/// policy creation operation. It contains only the essential information needed
/// by the caller.
///
/// # Example
///
/// ```rust,ignore
/// use policies::features::create_policy::dto::CreatedPolicyDto;
///
/// let dto = CreatedPolicyDto {
///     id: "policy-123".to_string(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedPolicyDto {
    /// The ID of the newly created policy
    pub id: String,
}

impl CreatedPolicyDto {
    /// Creates a new `CreatedPolicyDto`.
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PolicyContent Tests ---

    #[test]
    fn policy_content_accepts_valid_content() {
        let content = PolicyContent::new("permit(principal, action, resource);".to_string());
        assert!(content.is_ok());
    }

    #[test]
    fn policy_content_rejects_empty_string() {
        let content = PolicyContent::new("".to_string());
        assert!(content.is_err());
    }

    #[test]
    fn policy_content_rejects_whitespace_only() {
        let content = PolicyContent::new("   \n\t  ".to_string());
        assert!(content.is_err());
    }

    #[test]
    fn policy_content_as_ref_returns_inner_string() {
        let content = PolicyContent::new("test content".to_string()).unwrap();
        assert_eq!(content.as_ref(), "test content");
    }

    #[test]
    fn policy_content_display_returns_inner_string() {
        let content = PolicyContent::new("test content".to_string()).unwrap();
        assert_eq!(content.to_string(), "test content");
    }

    // --- CreatePolicyCommand Tests ---

    #[test]
    fn create_policy_command_with_valid_inputs() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Test policy".to_string()),
            Some(vec!["tag1".to_string(), "tag2".to_string()]),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn create_policy_command_with_minimal_inputs() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn create_policy_command_rejects_empty_content() {
        let result = CreatePolicyCommand::new("".to_string(), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("content"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[test]
    fn create_policy_command_rejects_empty_description() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("".to_string()),
            None,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("Description"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[test]
    fn create_policy_command_rejects_whitespace_only_description() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("   \n  ".to_string()),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn create_policy_command_rejects_empty_tags_list() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(Vec::new()),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("Tags"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[test]
    fn create_policy_command_rejects_duplicate_tags() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(vec![
                "tag1".to_string(),
                "tag2".to_string(),
                "tag1".to_string(),
            ]),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("duplicate"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[test]
    fn create_policy_command_accepts_none_description() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            None,
        );
        assert!(result.is_ok());
        let command = result.unwrap();
        assert!(command.description().is_none());
    }

    #[test]
    fn create_policy_command_accepts_none_tags() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Description".to_string()),
            None,
        );
        assert!(result.is_ok());
        let command = result.unwrap();
        assert!(command.tags().is_empty());
    }

    #[test]
    fn create_policy_command_getters_work() {
        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Test description".to_string()),
            Some(vec!["tag1".to_string(), "tag2".to_string()]),
        )
        .unwrap();

        assert_eq!(
            command.content().as_ref(),
            "permit(principal, action, resource);"
        );
        assert_eq!(command.description(), Some("Test description"));
        assert_eq!(command.tags(), &["tag1", "tag2"]);
    }

    #[test]
    fn create_policy_command_into_parts_works() {
        let command = CreatePolicyCommand::new(
            "test content".to_string(),
            Some("description".to_string()),
            Some(vec!["tag1".to_string()]),
        )
        .unwrap();

        let (content, description, tags) = command.into_parts();
        assert_eq!(content.as_ref(), "test content");
        assert_eq!(description, Some("description".to_string()));
        assert_eq!(tags, vec!["tag1".to_string()]);
    }

    // --- CreatedPolicyDto Tests ---

    #[test]
    fn created_policy_dto_can_be_created() {
        let dto = CreatedPolicyDto::new("policy-123");
        assert_eq!(dto.id, "policy-123");
    }

    #[test]
    fn created_policy_dto_serialization() {
        let dto = CreatedPolicyDto::new("policy-456");
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: CreatedPolicyDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, deserialized);
    }
}
