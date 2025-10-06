//! Infrastructure adapters for Update Policy feature
//!
//! This module contains stub/in-memory implementations of the update_policy ports.
//! These are primarily for testing and demonstration purposes.
//! Production implementations should use real database connections (e.g., SurrealDB).

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::Mutex;

use super::dto::{PolicyView, UpdatePolicyCommand};
use super::error::UpdatePolicyError;
use super::ports::UpdatePolicyPort;

/// In-memory adapter for UpdatePolicyPort
///
/// This is a simple in-memory implementation for testing purposes.
/// It stores policies in a HashMap protected by a Mutex.
///
/// # Thread Safety
///
/// This adapter is thread-safe and can be shared across threads using `Arc`.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// let adapter = Arc::new(InMemoryUpdatePolicyAdapter::new());
///
/// // Pre-populate with a policy
/// adapter.add_policy(policy);
///
/// // Use in UpdatePolicyUseCase
/// let use_case = UpdatePolicyUseCase::new(validator, adapter);
/// ```
pub struct InMemoryUpdatePolicyAdapter {
    policies: Mutex<HashMap<String, (String, Option<String>)>>, // id -> (content, description)
}

impl InMemoryUpdatePolicyAdapter {
    /// Create a new empty adapter
    pub fn new() -> Self {
        Self {
            policies: Mutex::new(HashMap::new()),
        }
    }

    /// Create an adapter pre-populated with policies
    pub fn with_policies(policies: Vec<(String, String, Option<String>)>) -> Self {
        let mut map = HashMap::new();
        for (id, content, description) in policies {
            map.insert(id, (content, description));
        }
        Self {
            policies: Mutex::new(map),
        }
    }

    /// Add a policy to the adapter (for testing setup)
    pub fn add_policy(&self, policy_id: String, content: String, description: Option<String>) {
        let mut policies = self.policies.lock().unwrap();
        policies.insert(policy_id, (content, description));
    }

    /// Get a policy from the adapter (for testing verification)
    pub fn get_policy(&self, policy_id: &str) -> Option<(String, Option<String>)> {
        let policies = self.policies.lock().unwrap();
        policies.get(policy_id).cloned()
    }

    /// Get the number of policies stored
    pub fn policy_count(&self) -> usize {
        let policies = self.policies.lock().unwrap();
        policies.len()
    }
}

impl Default for InMemoryUpdatePolicyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UpdatePolicyPort for InMemoryUpdatePolicyAdapter {
    async fn update(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
        let mut policies = self.policies.lock().unwrap();

        let (content, description) = policies
            .get_mut(&command.policy_id)
            .ok_or_else(|| UpdatePolicyError::PolicyNotFound(command.policy_id.clone()))?;

        if let Some(new_content) = command.policy_content {
            *content = new_content;
        }

        if let Some(new_description) = command.description {
            *description = if new_description.is_empty() {
                None
            } else {
                Some(new_description)
            };
        }

        Ok(PolicyView {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123456789012".to_string(),
                "Policy".to_string(),
                command.policy_id.clone(),
            ),
            name: command.policy_id.clone(),
            content: content.clone(),
            description: description.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_adapter_update_content() {
        // Arrange
        let adapter = InMemoryUpdatePolicyAdapter::new();
        adapter.add_policy(
            "test-policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            Some("Original description".to_string()),
        );

        // Act
        let command = UpdatePolicyCommand::update_content(
            "test-policy",
            "forbid(principal, action, resource);"
        );
        let result = adapter.update(command).await;

        // Assert
        assert!(result.is_ok());
        let updated = result.unwrap();
        assert!(updated.content.contains("forbid"));
        assert_eq!(updated.description, Some("Original description".to_string()));
    }

    #[tokio::test]
    async fn test_in_memory_adapter_update_description() {
        // Arrange
        let adapter = InMemoryUpdatePolicyAdapter::new();
        adapter.add_policy(
            "test-policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            Some("Original".to_string()),
        );

        // Act
        let command = UpdatePolicyCommand::update_description("test-policy", "Updated");
        let result = adapter.update(command).await;

        // Assert
        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.description, Some("Updated".to_string()));
        assert!(updated.content.contains("permit"));
    }

    #[tokio::test]
    async fn test_in_memory_adapter_not_found() {
        // Arrange
        let adapter = InMemoryUpdatePolicyAdapter::new();

        // Act
        let command = UpdatePolicyCommand::update_description("nonexistent", "Description");
        let result = adapter.update(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::PolicyNotFound(id) => {
                assert_eq!(id, "nonexistent");
            }
            e => panic!("Expected PolicyNotFound, got: {:?}", e),
        }
    }
}
