//! Use cases for IAM policy management
//!
//! This module implements CRUD operations for IAM policies following VSA principles.
//! Policy validation is delegated to the policies crate through a port interface,
//! maintaining the decoupling from Cedar implementation details.

use crate::features::create_policy::dto::{
    CreatePolicyCommand, DeletePolicyCommand, GetPolicyQuery, ListPoliciesQuery, PolicyDto,
    UpdatePolicyCommand,
};
use crate::features::create_policy::error::{
    CreatePolicyError, DeletePolicyError, GetPolicyError, ListPoliciesError, UpdatePolicyError,
};
use crate::features::create_policy::ports::{PolicyPersister, PolicyValidator};
use crate::shared::domain::{Hrn, Policy};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use tracing::instrument;

/// Use case for creating IAM policies
///
/// This use case validates policy content through a port and persists
/// the policy if validation succeeds.
pub struct CreatePolicyUseCase<P, V>
where
    P: PolicyPersister,
    V: PolicyValidator,
{
    policy_persister: Arc<P>,
    policy_validator: Arc<V>,
}

impl<P, V> CreatePolicyUseCase<P, V>
where
    P: PolicyPersister,
    V: PolicyValidator,
{
    pub fn new(policy_persister: Arc<P>, policy_validator: Arc<V>) -> Self {
        Self {
            policy_persister,
            policy_validator,
        }
    }

    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<PolicyDto, CreatePolicyError> {
        // Validate policy content through port
        let validation_result = self
            .policy_validator
            .validate_policy(&command.policy_content)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed(e.to_string()))?;

        if !validation_result.is_valid {
            let error_messages: Vec<String> = validation_result
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect();
            return Err(CreatePolicyError::InvalidPolicyContent(
                error_messages.join("; "),
            ));
        }

        // Create policy entity
        let hrn = Hrn::new("iam", "policy", &command.policy_id);
        let now = Utc::now();

        let policy = Policy {
            id: hrn.clone(),
            content: command.policy_content.clone(),
            description: command.description.clone(),
            created_at: now,
            updated_at: now,
        };

        // Persist policy
        let saved_policy = self.policy_persister.create_policy(command).await?;
        Ok(PolicyDto::from(saved_policy))
    }
}

/// Use case for deleting IAM policies
pub struct DeletePolicyUseCase<P: PolicyPersister> {
    policy_persister: Arc<P>,
}

impl<P: PolicyPersister> DeletePolicyUseCase<P> {
    pub fn new(policy_persister: Arc<P>) -> Self {
        Self { policy_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        self.policy_persister.delete_policy(command).await
    }
}

/// Use case for updating IAM policies
///
/// This use case validates the updated policy content before persisting changes.
pub struct UpdatePolicyUseCase<P, V>
where
    P: PolicyPersister,
    V: PolicyValidator,
{
    policy_persister: Arc<P>,
    policy_validator: Arc<V>,
}

impl<P, V> UpdatePolicyUseCase<P, V>
where
    P: PolicyPersister,
    V: PolicyValidator,
{
    pub fn new(policy_persister: Arc<P>, policy_validator: Arc<V>) -> Self {
        Self {
            policy_persister,
            policy_validator,
        }
    }

    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<PolicyDto, UpdatePolicyError> {
        // Validate policy content through port
        let validation_result = self
            .policy_validator
            .validate_policy(&command.policy_content)
            .await
            .map_err(|e| UpdatePolicyError::ValidationFailed(e.to_string()))?;

        if !validation_result.is_valid {
            let error_messages: Vec<String> = validation_result
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect();
            return Err(UpdatePolicyError::InvalidPolicyContent(
                error_messages.join("; "),
            ));
        }

        // Update policy
        let updated_policy = self.policy_persister.update_policy(command).await?;
        Ok(PolicyDto::from(updated_policy))
    }
}

/// Use case for retrieving a single IAM policy
pub struct GetPolicyUseCase<P: PolicyPersister> {
    policy_persister: Arc<P>,
}

impl<P: PolicyPersister> GetPolicyUseCase<P> {
    pub fn new(policy_persister: Arc<P>) -> Self {
        Self { policy_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyDto, GetPolicyError> {
        let policy = self.policy_persister.get_policy(query).await?;
        Ok(PolicyDto::from(policy))
    }
}

/// Use case for listing IAM policies
pub struct ListPoliciesUseCase<P: PolicyPersister> {
    policy_persister: Arc<P>,
}

impl<P: PolicyPersister> ListPoliciesUseCase<P> {
    pub fn new(policy_persister: Arc<P>) -> Self {
        Self { policy_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<Vec<PolicyDto>, ListPoliciesError> {
        let policies = self.policy_persister.list_policies(query).await?;
        Ok(policies.into_iter().map(PolicyDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Tests will use mocks from mocks.rs
}
