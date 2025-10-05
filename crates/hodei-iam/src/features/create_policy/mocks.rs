use crate::features::create_policy::dto::{
    CreatePolicyCommand, DeletePolicyCommand, GetPolicyQuery, ListPoliciesQuery,
    UpdatePolicyCommand,
};
use crate::features::create_policy::error::{
    CreatePolicyError, DeletePolicyError, GetPolicyError, ListPoliciesError, UpdatePolicyError,
};
use crate::features::create_policy::ports::{
    PolicyPersister, PolicyValidationError, PolicyValidator, ValidationError, ValidationResult,
    ValidationWarning,
};
use crate::shared::domain::Policy;
use crate::shared::domain::ports::{PolicyStorage, PolicyStorageError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct MockPolicyPersister {
    policies: Arc<Mutex<HashMap<String, Policy>>>,
}

impl MockPolicyPersister {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_policies(policies: HashMap<String, Policy>) -> Self {
        Self {
            policies: Arc::new(Mutex::new(policies)),
        }
    }
}

#[async_trait]
impl PolicyStorage for MockPolicyPersister {
    async fn save(&self, policy: Policy) -> Result<(), PolicyStorageError> {
        let mut policies = self.policies.lock().await;
        policies.insert(policy.id.to_string(), policy);
        Ok(())
    }

    async fn delete(&self, policy_id: &str) -> Result<(), PolicyStorageError> {
        let mut policies = self.policies.lock().await;
        policies.remove(policy_id);
        Ok(())
    }

    async fn update(&self, policy: Policy) -> Result<(), PolicyStorageError> {
        let mut policies = self.policies.lock().await;
        policies.insert(policy.id.to_string(), policy);
        Ok(())
    }

    async fn get(&self, policy_id: &str) -> Result<Option<Policy>, PolicyStorageError> {
        let policies = self.policies.lock().await;
        Ok(policies.get(policy_id).cloned())
    }

    async fn list(
        &self,
        _limit: Option<u32>,
        _offset: Option<u32>,
    ) -> Result<Vec<Policy>, PolicyStorageError> {
        let policies = self.policies.lock().await;
        Ok(policies.values().cloned().collect())
    }
}

#[async_trait]
impl PolicyPersister for MockPolicyPersister {
    async fn create_policy(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<Policy, CreatePolicyError> {
        let mut policies = self.policies.lock().await;
        if policies.contains_key(&command.policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists);
        }

        let policy = Policy {
            id: crate::shared::domain::Hrn::new("iam", "policy", &command.policy_id),
            content: command.policy_content,
            description: command.description,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        policies.insert(command.policy_id.clone(), policy.clone());
        Ok(policy)
    }

    async fn delete_policy(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        let mut policies = self.policies.lock().await;
        if !policies.contains_key(&command.policy_id) {
            return Err(DeletePolicyError::PolicyNotFound);
        }

        policies.remove(&command.policy_id);
        Ok(())
    }

    async fn update_policy(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<Policy, UpdatePolicyError> {
        let mut policies = self.policies.lock().await;
        let policy = policies
            .get_mut(&command.policy_id)
            .ok_or(UpdatePolicyError::PolicyNotFound)?;

        policy.content = command.policy_content;
        policy.description = command.description;
        policy.updated_at = chrono::Utc::now();

        Ok(policy.clone())
    }

    async fn get_policy(&self, query: GetPolicyQuery) -> Result<Policy, GetPolicyError> {
        let policies = self.policies.lock().await;
        policies
            .get(&query.policy_id)
            .cloned()
            .ok_or(GetPolicyError::PolicyNotFound)
    }

    async fn list_policies(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<Vec<Policy>, ListPoliciesError> {
        let policies = self.policies.lock().await;
        let mut result: Vec<Policy> = policies.values().cloned().collect();

        // Apply limit and offset if specified
        if let Some(offset) = query.offset {
            result = result.into_iter().skip(offset as usize).collect();
        }

        if let Some(limit) = query.limit {
            result = result.into_iter().take(limit as usize).collect();
        }

        Ok(result)
    }
}

/// Mock implementation of PolicyValidator for testing
///
/// This mock can be configured to return success or failure for validation,
/// allowing tests to exercise different validation scenarios.
pub struct MockPolicyValidator {
    should_succeed: Arc<Mutex<bool>>,
    validation_errors: Arc<Mutex<Vec<String>>>,
}

impl MockPolicyValidator {
    /// Create a new mock that always succeeds validation
    pub fn new_always_valid() -> Self {
        Self {
            should_succeed: Arc::new(Mutex::new(true)),
            validation_errors: Arc::new(Mutex::new(vec![])),
        }
    }

    /// Create a new mock that always fails validation with the given errors
    pub fn new_always_invalid(errors: Vec<String>) -> Self {
        Self {
            should_succeed: Arc::new(Mutex::new(false)),
            validation_errors: Arc::new(Mutex::new(errors)),
        }
    }

    /// Create a new mock with custom behavior
    pub fn new(should_succeed: bool, errors: Vec<String>) -> Self {
        Self {
            should_succeed: Arc::new(Mutex::new(should_succeed)),
            validation_errors: Arc::new(Mutex::new(errors)),
        }
    }

    /// Set whether the next validation should succeed
    pub async fn set_should_succeed(&self, should_succeed: bool) {
        let mut s = self.should_succeed.lock().await;
        *s = should_succeed;
    }

    /// Set the validation errors to return on failure
    pub async fn set_validation_errors(&self, errors: Vec<String>) {
        let mut e = self.validation_errors.lock().await;
        *e = errors;
    }
}

#[async_trait]
impl PolicyValidator for MockPolicyValidator {
    async fn validate_policy(
        &self,
        _policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        let should_succeed = *self.should_succeed.lock().await;
        let errors = self.validation_errors.lock().await.clone();

        if should_succeed {
            Ok(ValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
            })
        } else {
            Ok(ValidationResult {
                is_valid: false,
                errors: errors
                    .into_iter()
                    .map(|msg| ValidationError {
                        message: msg,
                        line: None,
                        column: None,
                    })
                    .collect(),
                warnings: vec![],
            })
        }
    }
}
