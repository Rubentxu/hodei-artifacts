//! # Mock Implementations for Create Policy Feature Testing
//!
//! This module provides mock implementations of the ports defined in the `create_policy`
//! feature. These mocks are designed for use in unit tests, allowing for controlled
//! testing scenarios without requiring real external dependencies.
//!
//! ## Mock Implementations
//!
//! - **`MockPolicyIdGenerator`**: A configurable mock that can generate predetermined IDs
//!   or simulate failures.
//!
//! - **`MockPolicyValidator`**: A mock validator that can be configured to pass, fail with
//!   specific errors, or validate based on custom logic.
//!
//! - **`MockPolicyPersister`**: A mock persister that tracks save operations and can be
//!   configured to succeed or fail with specific errors.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::features::create_policy::mocks::*;
//!
//! let id_gen = MockPolicyIdGenerator::new_with_id("test-id-123");
//! let validator = MockPolicyValidator::new_accepting_all();
//! let persister = MockPolicyPersister::new();
//! ```

use crate::features::create_policy::dto::PolicyContent;
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{PolicyIdGenerator, PolicyPersister, PolicyValidator};
use crate::shared::domain::policy::{Policy, PolicyId};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

// --- Mock Policy ID Generator ---

/// A mock implementation of `PolicyIdGenerator` for testing.
///
/// This mock can be configured to:
/// - Return a predetermined ID
/// - Return a sequence of IDs
/// - Simulate generation failures
#[derive(Debug, Clone)]
pub struct MockPolicyIdGenerator {
    behavior: Arc<Mutex<IdGeneratorBehavior>>,
}

#[derive(Debug)]
enum IdGeneratorBehavior {
    /// Always return the same fixed ID
    Fixed(String),
    /// Return IDs from a sequence
    Sequence(Vec<String>, usize),
    /// Always fail with a specific error
    Error(String),
}

impl MockPolicyIdGenerator {
    /// Creates a mock that always returns the same ID.
    pub fn new_with_id(id: impl Into<String>) -> Self {
        Self {
            behavior: Arc::new(Mutex::new(IdGeneratorBehavior::Fixed(id.into()))),
        }
    }

    /// Creates a mock that returns IDs from a sequence.
    pub fn new_with_sequence(ids: Vec<String>) -> Self {
        Self {
            behavior: Arc::new(Mutex::new(IdGeneratorBehavior::Sequence(ids, 0))),
        }
    }

    /// Creates a mock that always fails with the given error message.
    pub fn new_failing(error_msg: impl Into<String>) -> Self {
        Self {
            behavior: Arc::new(Mutex::new(IdGeneratorBehavior::Error(error_msg.into()))),
        }
    }
}

#[async_trait]
impl PolicyIdGenerator for MockPolicyIdGenerator {
    async fn generate(&self) -> Result<PolicyId, CreatePolicyError> {
        let mut behavior = self.behavior.lock().unwrap();
        match &mut *behavior {
            IdGeneratorBehavior::Fixed(id) => Ok(PolicyId::new(id.clone())),
            IdGeneratorBehavior::Sequence(ids, index) => {
                if *index >= ids.len() {
                    return Err(CreatePolicyError::Internal(
                        "Mock sequence exhausted".to_string(),
                    ));
                }
                let id = ids[*index].clone();
                *index += 1;
                Ok(PolicyId::new(id))
            }
            IdGeneratorBehavior::Error(msg) => Err(CreatePolicyError::Internal(msg.clone())),
        }
    }
}

// --- Mock Policy Validator ---

/// A mock implementation of `PolicyValidator` for testing.
///
/// This mock can be configured to:
/// - Accept all policies as valid
/// - Reject all policies with a specific error
/// - Use custom validation logic
#[derive(Debug, Clone)]
pub struct MockPolicyValidator {
    behavior: Arc<Mutex<ValidatorBehavior>>,
}

enum ValidatorBehavior {
    /// Accept all policies
    AcceptAll,
    /// Reject all policies with a specific error message
    RejectAll(String),
    /// Track validation calls
    Tracking(Vec<String>),
    /// Custom validation function
    Custom(Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>),
}

impl std::fmt::Debug for ValidatorBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AcceptAll => write!(f, "AcceptAll"),
            Self::RejectAll(msg) => f.debug_tuple("RejectAll").field(msg).finish(),
            Self::Tracking(policies) => f.debug_tuple("Tracking").field(policies).finish(),
            Self::Custom(_) => write!(f, "Custom(<function>)"),
        }
    }
}

impl MockPolicyValidator {
    /// Creates a mock that accepts all policies as valid.
    pub fn new_accepting_all() -> Self {
        Self {
            behavior: Arc::new(Mutex::new(ValidatorBehavior::AcceptAll)),
        }
    }

    /// Creates a mock that rejects all policies with the given error.
    pub fn new_rejecting_all(error_msg: impl Into<String>) -> Self {
        Self {
            behavior: Arc::new(Mutex::new(ValidatorBehavior::RejectAll(error_msg.into()))),
        }
    }

    /// Creates a mock that tracks all validation calls.
    pub fn new_tracking() -> Self {
        Self {
            behavior: Arc::new(Mutex::new(ValidatorBehavior::Tracking(Vec::new()))),
        }
    }

    /// Creates a mock with custom validation logic.
    pub fn new_with_custom<F>(validator_fn: F) -> Self
    where
        F: Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    {
        Self {
            behavior: Arc::new(Mutex::new(ValidatorBehavior::Custom(Arc::new(
                validator_fn,
            )))),
        }
    }

    /// Gets the list of validated policy contents (only for tracking mode).
    pub fn get_validated_policies(&self) -> Vec<String> {
        let behavior = self.behavior.lock().unwrap();
        match &*behavior {
            ValidatorBehavior::Tracking(policies) => policies.clone(),
            _ => Vec::new(),
        }
    }
}

#[async_trait]
impl PolicyValidator for MockPolicyValidator {
    async fn validate(&self, content: &PolicyContent) -> Result<(), CreatePolicyError> {
        let mut behavior = self.behavior.lock().unwrap();
        match &mut *behavior {
            ValidatorBehavior::AcceptAll => Ok(()),
            ValidatorBehavior::RejectAll(msg) => {
                Err(CreatePolicyError::ValidationError(msg.clone()))
            }
            ValidatorBehavior::Tracking(policies) => {
                policies.push(content.as_ref().to_string());
                Ok(())
            }
            ValidatorBehavior::Custom(validator_fn) => {
                validator_fn(content.as_ref()).map_err(CreatePolicyError::ValidationError)
            }
        }
    }
}

// --- Mock Policy Persister ---

/// A mock implementation of `PolicyPersister` for testing.
///
/// This mock tracks all save operations and can be configured to:
/// - Successfully save all policies
/// - Fail with specific errors
/// - Simulate conflicts
#[derive(Debug, Clone)]
pub struct MockPolicyPersister {
    state: Arc<Mutex<PersisterState>>,
}

#[derive(Debug)]
struct PersisterState {
    saved_policies: Vec<Policy>,
    behavior: PersisterBehavior,
}

#[derive(Debug)]
enum PersisterBehavior {
    /// Successfully save all policies
    Success,
    /// Always fail with a specific error
    Error(CreatePolicyError),
    /// Simulate conflict for specific policy IDs
    ConflictOn(Vec<String>),
}

impl MockPolicyPersister {
    /// Creates a mock persister that succeeds for all operations.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(PersisterState {
                saved_policies: Vec::new(),
                behavior: PersisterBehavior::Success,
            })),
        }
    }

    /// Creates a mock persister that always fails with the given error.
    pub fn new_failing(error: CreatePolicyError) -> Self {
        Self {
            state: Arc::new(Mutex::new(PersisterState {
                saved_policies: Vec::new(),
                behavior: PersisterBehavior::Error(error),
            })),
        }
    }

    /// Creates a mock persister that simulates conflicts for specific policy IDs.
    pub fn new_with_conflicts(conflicting_ids: Vec<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(PersisterState {
                saved_policies: Vec::new(),
                behavior: PersisterBehavior::ConflictOn(conflicting_ids),
            })),
        }
    }

    /// Returns a list of all policies that were saved.
    pub fn get_saved_policies(&self) -> Vec<Policy> {
        let state = self.state.lock().unwrap();
        state.saved_policies.clone()
    }

    /// Returns the number of policies that were saved.
    pub fn save_count(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.saved_policies.len()
    }

    /// Checks if a policy with the given ID was saved.
    pub fn was_saved(&self, policy_id: &PolicyId) -> bool {
        let state = self.state.lock().unwrap();
        state.saved_policies.iter().any(|p| p.id() == policy_id)
    }
}

impl Default for MockPolicyPersister {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyPersister for MockPolicyPersister {
    async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError> {
        let mut state = self.state.lock().unwrap();

        match &state.behavior {
            PersisterBehavior::Success => {
                state.saved_policies.push(policy.clone());
                Ok(())
            }
            PersisterBehavior::Error(err) => Err(err.clone()),
            PersisterBehavior::ConflictOn(ids) => {
                if ids.contains(&policy.id().to_string()) {
                    Err(CreatePolicyError::Conflict(policy.id().to_string()))
                } else {
                    state.saved_policies.push(policy.clone());
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_id_generator_fixed_returns_same_id() {
        let mock = MockPolicyIdGenerator::new_with_id("test-123");
        let id1 = mock.generate().await.unwrap();
        let id2 = mock.generate().await.unwrap();
        assert_eq!(id1, id2);
        assert_eq!(id1.to_string(), "test-123");
    }

    #[tokio::test]
    async fn mock_id_generator_sequence_returns_in_order() {
        let mock = MockPolicyIdGenerator::new_with_sequence(vec![
            "id-1".to_string(),
            "id-2".to_string(),
            "id-3".to_string(),
        ]);
        assert_eq!(mock.generate().await.unwrap().to_string(), "id-1");
        assert_eq!(mock.generate().await.unwrap().to_string(), "id-2");
        assert_eq!(mock.generate().await.unwrap().to_string(), "id-3");
    }

    #[tokio::test]
    async fn mock_id_generator_failing_returns_error() {
        let mock = MockPolicyIdGenerator::new_failing("generation failed");
        let result = mock.generate().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_validator_accepting_all_accepts() {
        let mock = MockPolicyValidator::new_accepting_all();
        let content =
            PolicyContent::new("permit(principal, action, resource);".to_string()).unwrap();
        assert!(mock.validate(&content).await.is_ok());
    }

    #[tokio::test]
    async fn mock_validator_rejecting_all_rejects() {
        let mock = MockPolicyValidator::new_rejecting_all("invalid");
        let content =
            PolicyContent::new("permit(principal, action, resource);".to_string()).unwrap();
        let result = mock.validate(&content).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_validator_tracking_records_validations() {
        let mock = MockPolicyValidator::new_tracking();
        let content1 = PolicyContent::new("policy1".to_string()).unwrap();
        let content2 = PolicyContent::new("policy2".to_string()).unwrap();

        mock.validate(&content1).await.unwrap();
        mock.validate(&content2).await.unwrap();

        let validated = mock.get_validated_policies();
        assert_eq!(validated.len(), 2);
        assert_eq!(validated[0], "policy1");
        assert_eq!(validated[1], "policy2");
    }

    #[tokio::test]
    async fn mock_persister_tracks_saved_policies() {
        use crate::shared::domain::policy::Policy;

        let mock = MockPolicyPersister::new();
        let policy = Policy::new_without_metadata(
            PolicyId::new("test-id"),
            "permit(principal, action, resource);".to_string(),
        );

        mock.save(&policy).await.unwrap();

        assert_eq!(mock.save_count(), 1);
        assert!(mock.was_saved(policy.id()));
        let saved = mock.get_saved_policies();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id(), policy.id());
    }

    #[tokio::test]
    async fn mock_persister_failing_returns_error() {
        use crate::shared::domain::policy::Policy;

        let mock =
            MockPolicyPersister::new_failing(CreatePolicyError::Internal("db error".to_string()));
        let policy = Policy::new_without_metadata(
            PolicyId::new("test-id"),
            "permit(principal, action, resource);".to_string(),
        );

        let result = mock.save(&policy).await;
        assert!(result.is_err());
        assert_eq!(mock.save_count(), 0);
    }

    #[tokio::test]
    async fn mock_persister_with_conflicts_rejects_specific_ids() {
        use crate::shared::domain::policy::Policy;

        let mock = MockPolicyPersister::new_with_conflicts(vec!["conflict-id".to_string()]);

        let policy1 = Policy::new_without_metadata(
            PolicyId::new("conflict-id"),
            "permit(principal, action, resource);".to_string(),
        );
        let policy2 = Policy::new_without_metadata(
            PolicyId::new("ok-id"),
            "permit(principal, action, resource);".to_string(),
        );

        let result1 = mock.save(&policy1).await;
        assert!(result1.is_err());

        let result2 = mock.save(&policy2).await;
        assert!(result2.is_ok());

        assert_eq!(mock.save_count(), 1);
        assert!(!mock.was_saved(&PolicyId::new("conflict-id")));
        assert!(mock.was_saved(&PolicyId::new("ok-id")));
    }
}
