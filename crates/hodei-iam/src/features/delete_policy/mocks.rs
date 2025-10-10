//! Mock implementations for testing the delete_policy feature
//!
//! This module provides mock implementations of the ports used by
//! DeletePolicyUseCase, allowing for isolated unit testing without
//! requiring real infrastructure (databases, etc.)

use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::ports::DeletePolicyPort;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Mock implementation of DeletePolicyPort for testing
///
/// This mock allows tests to:
/// - Configure success/failure scenarios
/// - Track which policies were deleted
/// - Simulate storage errors
/// - Simulate policy not found errors
/// - Simulate policy in use errors
/// - Simulate system policy protection
#[derive(Debug)]
pub struct MockDeletePolicyPort {
    /// If true, delete() will fail with a storage error
    pub should_fail_storage: bool,

    /// If true, delete() will fail with PolicyNotFound error
    pub should_fail_not_found: bool,

    /// If true, delete() will fail with PolicyInUse error
    pub should_fail_in_use: bool,

    /// If true, delete() will fail with SystemPolicyProtected error
    pub should_fail_system_protected: bool,

    /// Policy IDs that exist in the system
    pub existing_policy_ids: Arc<Mutex<HashSet<String>>>,

    /// Policy IDs that are marked as "in use" and cannot be deleted
    pub in_use_policy_ids: Vec<String>,

    /// Policy IDs that are system-protected and cannot be deleted
    pub system_policy_ids: Vec<String>,

    /// List of policies that were successfully deleted
    pub deleted_policies: Arc<Mutex<Vec<String>>>,

    /// Counter tracking how many times delete was called
    pub call_count: Arc<Mutex<usize>>,
}

impl Default for MockDeletePolicyPort {
    fn default() -> Self {
        Self {
            should_fail_storage: false,
            should_fail_not_found: false,
            should_fail_in_use: false,
            should_fail_system_protected: false,
            existing_policy_ids: Arc::new(Mutex::new(HashSet::new())),
            in_use_policy_ids: vec![],
            system_policy_ids: vec![],
            deleted_policies: Arc::new(Mutex::new(vec![])),
            call_count: Arc::new(Mutex::new(0)),
        }
    }
}

impl MockDeletePolicyPort {
    /// Create a new mock port that will succeed (with no existing policies)
    pub fn new() -> Self {
        let port = Self::default();
        port.add_policy("test-policy".to_string());
        port
    }

    /// Create a mock that will fail with a storage error
    pub fn with_storage_error() -> Self {
        Self {
            should_fail_storage: true,
            ..Default::default()
        }
    }

    /// Create a mock that will fail with PolicyNotFound error
    pub fn with_not_found_error() -> Self {
        Self {
            should_fail_not_found: true,
            ..Default::default()
        }
    }

    /// Create a mock that will fail with PolicyInUse error
    pub fn with_in_use_error() -> Self {
        Self {
            should_fail_in_use: true,
            ..Default::default()
        }
    }

    /// Create a mock that will fail with SystemPolicyProtected error
    pub fn with_system_protected_error() -> Self {
        Self {
            should_fail_system_protected: true,
            ..Default::default()
        }
    }

    /// Create a mock with pre-existing policy IDs
    pub fn with_existing_policies(policy_ids: Vec<String>) -> Self {
        let mut set = HashSet::new();
        for id in policy_ids {
            set.insert(id);
        }
        Self {
            existing_policy_ids: Arc::new(Mutex::new(set)),
            ..Default::default()
        }
    }

    /// Create a mock with policies that are in use
    pub fn with_in_use_policies(policy_ids: Vec<String>) -> Self {
        let mut existing = HashSet::new();
        for id in &policy_ids {
            existing.insert(id.clone());
        }
        Self {
            existing_policy_ids: Arc::new(Mutex::new(existing)),
            in_use_policy_ids: policy_ids,
            ..Default::default()
        }
    }

    /// Create a mock with system-protected policies
    pub fn with_system_policies(policy_ids: Vec<String>) -> Self {
        let mut existing = HashSet::new();
        for id in &policy_ids {
            existing.insert(id.clone());
        }
        Self {
            existing_policy_ids: Arc::new(Mutex::new(existing)),
            system_policy_ids: policy_ids,
            ..Default::default()
        }
    }

    /// Add a policy to the existing set
    pub fn add_policy(&self, policy_id: String) {
        self.existing_policy_ids.lock().unwrap().insert(policy_id);
    }

    /// Get the number of successfully deleted policies
    pub fn get_deleted_count(&self) -> usize {
        self.deleted_policies.lock().unwrap().len()
    }

    /// Get the number of times delete was called
    pub fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    /// Get a clone of all deleted policy IDs
    pub fn get_deleted_policies(&self) -> Vec<String> {
        self.deleted_policies.lock().unwrap().clone()
    }

    /// Check if a specific policy ID was deleted
    pub fn was_deleted(&self, policy_id: &str) -> bool {
        self.deleted_policies
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == policy_id)
    }

    /// Check if a policy exists
    pub fn exists(&self, policy_id: &str) -> bool {
        self.existing_policy_ids.lock().unwrap().contains(policy_id)
    }
}

#[async_trait]
impl DeletePolicyPort for MockDeletePolicyPort {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        // Increment call counter
        *self.call_count.lock().unwrap() += 1;

        // Simulate storage error if configured
        if self.should_fail_storage {
            return Err(DeletePolicyError::StorageError(
                "Mock storage error: database connection failed".to_string(),
            ));
        }

        // Simulate not found error if configured
        if self.should_fail_not_found {
            return Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()));
        }

        // Simulate in use error if configured
        if self.should_fail_in_use {
            return Err(DeletePolicyError::PolicyInUse(format!(
                "Policy '{}' is attached to users/groups",
                policy_id
            )));
        }

        // Simulate system protected error if configured
        if self.should_fail_system_protected {
            return Err(DeletePolicyError::SystemPolicyProtected(
                policy_id.to_string(),
            ));
        }

        // Check if policy is system-protected
        if self.system_policy_ids.contains(&policy_id.to_string()) {
            return Err(DeletePolicyError::SystemPolicyProtected(
                policy_id.to_string(),
            ));
        }

        // Check if policy exists
        let exists = self.existing_policy_ids.lock().unwrap().contains(policy_id);

        if !exists {
            return Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()));
        }

        // Check if policy is in use
        if self.in_use_policy_ids.contains(&policy_id.to_string()) {
            return Err(DeletePolicyError::PolicyInUse(format!(
                "Policy '{}' is attached to users/groups",
                policy_id
            )));
        }

        // Remove from existing policies
        self.existing_policy_ids.lock().unwrap().remove(policy_id);

        // Add to deleted list
        self.deleted_policies
            .lock()
            .unwrap()
            .push(policy_id.to_string());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_port_success() {
        let port = MockDeletePolicyPort::with_existing_policies(vec!["test-policy".to_string()]);

        let result = port.delete("test-policy").await;
        assert!(result.is_ok());
        assert_eq!(port.get_deleted_count(), 1);
        assert_eq!(port.get_call_count(), 1);
        assert!(port.was_deleted("test-policy"));
        assert!(!port.exists("test-policy"));
    }

    #[tokio::test]
    async fn test_mock_port_not_found() {
        let port = MockDeletePolicyPort::new();

        let result = port.delete("non-existent").await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), DeletePolicyError::PolicyNotFound(_));
        assert_eq!(port.get_deleted_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_port_storage_error() {
        let port = MockDeletePolicyPort::with_storage_error();

        let result = port.delete("test-policy").await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), DeletePolicyError::StorageError(_));
    }

    #[tokio::test]
    async fn test_mock_port_in_use_error() {
        let port = MockDeletePolicyPort::with_in_use_policies(vec!["in-use-policy".to_string()]);

        let result = port.delete("in-use-policy").await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), DeletePolicyError::PolicyInUse(_));
        assert_eq!(port.get_deleted_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_port_system_protected() {
        let port = MockDeletePolicyPort::with_system_policies(vec!["system-policy".to_string()]);

        let result = port.delete("system-policy").await;
        assert!(result.is_err());
        matches!(
            result.unwrap_err(),
            DeletePolicyError::SystemPolicyProtected(_)
        );
        assert_eq!(port.get_deleted_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_port_multiple_deletes() {
        let port = MockDeletePolicyPort::with_existing_policies(vec![
            "policy1".to_string(),
            "policy2".to_string(),
            "policy3".to_string(),
        ]);

        port.delete("policy1").await.unwrap();
        port.delete("policy2").await.unwrap();

        assert_eq!(port.get_deleted_count(), 2);
        assert_eq!(port.get_call_count(), 2);
        assert!(port.was_deleted("policy1"));
        assert!(port.was_deleted("policy2"));
        assert!(!port.was_deleted("policy3"));
        assert!(port.exists("policy3"));
    }

    #[tokio::test]
    async fn test_mock_port_add_policy() {
        let port = MockDeletePolicyPort::new();
        port.add_policy("new-policy".to_string());

        assert!(port.exists("new-policy"));
        port.delete("new-policy").await.unwrap();
        assert!(!port.exists("new-policy"));
    }

    #[tokio::test]
    async fn test_mock_port_forced_errors() {
        let port = MockDeletePolicyPort::with_not_found_error();
        let result = port.delete("anything").await;
        assert!(result.is_err());

        let port = MockDeletePolicyPort::with_in_use_error();
        let result = port.delete("anything").await;
        assert!(result.is_err());
    }
}
