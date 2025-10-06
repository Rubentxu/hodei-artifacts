//! Mock implementations for testing the evaluate_iam_policies feature
//!
//! This module provides mock implementations of the ports used by the
//! evaluate_iam_policies use case, facilitating unit testing without
//! requiring real infrastructure.

use async_trait::async_trait;
use cedar_policy::PolicySet;
use kernel::Hrn;

use super::ports::{PolicyFinderError, PolicyFinderPort};

/// Mock implementation of PolicyFinderPort for testing
///
/// This mock allows tests to control what policies are returned,
/// enabling testing of different scenarios:
/// - Empty policy sets (implicit deny)
/// - Specific policy sets (test allow/deny logic)
/// - Error conditions (test error handling)
///
/// # Examples
///
/// ```rust,ignore
/// use crate::features::evaluate_iam_policies::mocks::MockPolicyFinder;
/// use cedar_policy::PolicySet;
///
/// // Return specific policies
/// let policy_set = PolicySet::from_str("permit(principal, action, resource);").unwrap();
/// let mock = MockPolicyFinder::new(policy_set);
///
/// // Simulate errors
/// let mock = MockPolicyFinder::with_error("Database error".to_string());
/// ```
pub struct MockPolicyFinder {
    /// The policy set to return (if no error)
    policy_set: Option<PolicySet>,
    /// Error to return (if set)
    error: Option<String>,
}

impl MockPolicyFinder {
    /// Create a new mock that returns the given policy set
    ///
    /// # Arguments
    ///
    /// * `policy_set` - The PolicySet to return from get_effective_policies
    pub fn new(policy_set: PolicySet) -> Self {
        Self {
            policy_set: Some(policy_set),
            error: None,
        }
    }

    /// Create a new mock that returns an error
    ///
    /// # Arguments
    ///
    /// * `error` - The error message to return
    pub fn with_error(error: String) -> Self {
        Self {
            policy_set: None,
            error: Some(error),
        }
    }

    /// Create a new mock that returns an empty policy set
    pub fn empty() -> Self {
        Self::new(PolicySet::new())
    }
}

#[async_trait]
impl PolicyFinderPort for MockPolicyFinder {
    async fn get_effective_policies(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<PolicySet, PolicyFinderError> {
        if let Some(error_msg) = &self.error {
            return Err(PolicyFinderError::RepositoryError(error_msg.clone()));
        }

        Ok(self.policy_set.clone().unwrap_or_else(PolicySet::new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_mock_returns_configured_policy_set() {
        let policy_text = "permit(principal, action, resource);";
        let policy_set = PolicySet::from_str(policy_text).unwrap();
        let mock = MockPolicyFinder::new(policy_set.clone());

        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        let result = mock.get_effective_policies(&principal_hrn).await;

        assert!(result.is_ok());
        let returned_set = result.unwrap();
        assert_eq!(
            returned_set.policies().count(),
            policy_set.policies().count()
        );
    }

    #[tokio::test]
    async fn test_mock_returns_empty_policy_set() {
        let mock = MockPolicyFinder::empty();

        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        let result = mock.get_effective_policies(&principal_hrn).await;

        assert!(result.is_ok());
        let returned_set = result.unwrap();
        assert_eq!(returned_set.policies().count(), 0);
    }

    #[tokio::test]
    async fn test_mock_returns_configured_error() {
        let error_msg = "Database connection failed".to_string();
        let mock = MockPolicyFinder::with_error(error_msg.clone());

        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        let result = mock.get_effective_policies(&principal_hrn).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, PolicyFinderError::RepositoryError(_)));
        assert_eq!(
            error.to_string(),
            format!("Repository error: {}", error_msg)
        );
    }
}
