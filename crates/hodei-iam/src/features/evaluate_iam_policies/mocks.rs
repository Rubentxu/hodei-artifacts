//! Mock implementations for testing evaluate_iam_policies feature
//!
//! This module provides mock implementations of the ports used by the
//! evaluate_iam_policies use case, enabling isolated unit testing.

use async_trait::async_trait;
use kernel::application::ports::authorization::AuthorizationError;
use kernel::domain::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::ports::PolicyFinderPort;

/// Mock implementation of PolicyFinderPort for testing
///
/// This mock allows tests to configure which policies should be returned
/// for specific principals, enabling various test scenarios.
#[derive(Clone)]
pub struct MockPolicyFinder {
    /// Map of principal HRN to policy documents
    policies: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl MockPolicyFinder {
    /// Create a new mock policy finder with no policies
    pub fn new() -> Self {
        Self {
            policies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a mock that returns specific policies for any principal
    pub fn with_policies(policies: Vec<String>) -> Self {
        let mut map = HashMap::new();
        map.insert("*".to_string(), policies);
        Self {
            policies: Arc::new(Mutex::new(map)),
        }
    }

    /// Configure policies for a specific principal
    pub fn add_policies_for_principal(&self, principal_hrn: &str, policies: Vec<String>) {
        let mut map = self.policies.lock().unwrap();
        map.insert(principal_hrn.to_string(), policies);
    }

    /// Clear all configured policies
    pub fn clear(&self) {
        let mut map = self.policies.lock().unwrap();
        map.clear();
    }
}

impl Default for MockPolicyFinder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for MockPolicyFinder {
    async fn get_policies_for_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, AuthorizationError> {
        let map = self.policies.lock().unwrap();

        // First try exact match
        if let Some(policies) = map.get(&principal_hrn.to_string()) {
            return Ok(policies.clone());
        }

        // Fall back to wildcard
        if let Some(policies) = map.get("*") {
            return Ok(policies.clone());
        }

        // No policies configured
        Ok(Vec::new())
    }
}

/// Mock that always returns an error
pub struct MockPolicyFinderWithError {
    error_message: String,
}

impl MockPolicyFinderWithError {
    pub fn new(error_message: String) -> Self {
        Self { error_message }
    }
}

#[async_trait]
impl PolicyFinderPort for MockPolicyFinderWithError {
    async fn get_policies_for_principal(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<Vec<String>, AuthorizationError> {
        Err(AuthorizationError::EvaluationFailed(
            self.error_message.clone(),
        ))
    }
}

/// Mock that returns empty policies (for implicit deny testing)
pub struct MockEmptyPolicyFinder;

impl MockEmptyPolicyFinder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockEmptyPolicyFinder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for MockEmptyPolicyFinder {
    async fn get_policies_for_principal(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<Vec<String>, AuthorizationError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_policy_finder_empty() {
        let mock = MockPolicyFinder::new();
        let hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        let result = mock.get_policies_for_principal(&hrn).await.unwrap();
        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_mock_policy_finder_with_policies() {
        let policies = vec!["permit(principal, action, resource);".to_string()];
        let mock = MockPolicyFinder::with_policies(policies.clone());

        let hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        let result = mock.get_policies_for_principal(&hrn).await.unwrap();
        assert_eq!(result, policies);
    }

    #[tokio::test]
    async fn test_mock_policy_finder_specific_principal() {
        let mock = MockPolicyFinder::new();

        let alice_hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        let bob_hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "bob".to_string(),
        );

        mock.add_policies_for_principal(
            &alice_hrn.to_string(),
            vec!["permit(principal == Iam::User::\"alice\", action, resource);".to_string()],
        );

        let alice_result = mock.get_policies_for_principal(&alice_hrn).await.unwrap();
        assert_eq!(alice_result.len(), 1);

        let bob_result = mock.get_policies_for_principal(&bob_hrn).await.unwrap();
        assert_eq!(bob_result.len(), 0);
    }

    #[tokio::test]
    async fn test_mock_policy_finder_with_error() {
        let mock = MockPolicyFinderWithError::new("Database connection failed".to_string());

        let hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        let result = mock.get_policies_for_principal(&hrn).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_empty_policy_finder() {
        let mock = MockEmptyPolicyFinder::new();

        let hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        let result = mock.get_policies_for_principal(&hrn).await.unwrap();
        assert_eq!(result.len(), 0);
    }
}
