// crates/iam/src/features/update_policy/dto.rs

use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use cedar_policy::PolicyId;
use serde::{Deserialize, Serialize};

/// Command to update an existing policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyCommand {
    /// ID of the policy to update
    pub id: PolicyId,
    /// New name for the policy (optional)
    pub name: Option<String>,
    /// New description for the policy (optional)
    pub description: Option<String>,
    /// New Cedar policy content (optional)
    pub content: Option<String>,
    /// Tags to add or replace (optional)
    pub tags: Option<Vec<String>>,
    /// User performing the update
    pub updated_by: String,
}

/// Response after updating a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyResponse {
    /// The updated policy
    pub policy: Policy,
    /// Success message
    pub message: String,
}

impl UpdatePolicyCommand {
    /// Create a new update policy command
    pub fn new(id: PolicyId, updated_by: String) -> Self {
        Self {
            id,
            name: None,
            description: None,
            content: None,
            tags: None,
            updated_by,
        }
    }

    /// Set the new name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set the new description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Set the new content
    pub fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    /// Set the new tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Validate the command
    pub fn validate(&self) -> Result<(), IamError> {
        // Check if at least one field is being updated
        if self.name.is_none()
            && self.description.is_none()
            && self.content.is_none()
            && self.tags.is_none()
        {
            return Err(IamError::InvalidInput(
                "At least one field must be updated".to_string(),
            ));
        }

        // Validate name if provided
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err(IamError::InvalidInput("Name cannot be empty".to_string()));
            }
            if name.len() > 255 {
                return Err(IamError::InvalidInput(
                    "Name cannot exceed 255 characters".to_string(),
                ));
            }
        }

        // Validate content if provided
        if let Some(content) = &self.content {
            if content.trim().is_empty() {
                return Err(IamError::InvalidInput(
                    "Content cannot be empty".to_string(),
                ));
            }
        }

        // Validate updated_by
        if self.updated_by.trim().is_empty() {
            return Err(IamError::InvalidInput(
                "Updated by field cannot be empty".to_string(),
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

impl UpdatePolicyResponse {
    /// Create a new update policy response
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            message: "Policy updated successfully".to_string(),
        }
    }
}
