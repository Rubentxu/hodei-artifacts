//! Use case for creating IAM policies
//!
//! This module implements the business logic for creating new IAM policies.
//! Following Clean Architecture and Vertical Slice Architecture (VSA) principles,
//! this use case is self-contained and depends only on abstract ports.
//!
//! # Flow
//!
//! 1. Receive `CreatePolicyCommand` from the caller
//! 2. Validate policy content through `PolicyValidator` port
//! 3. If valid, persist through `CreatePolicyPort`
//! 4. Return `PolicyView` DTO with created policy details
//!
//! # Dependencies
//!
//! - `PolicyValidator`: Abstract port for Cedar policy validation
//! - `CreatePolicyPort`: Abstract port for policy persistence (ISP - only create)

use crate::features::create_policy::dto::{CreatePolicyCommand, PolicyView};
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{
    CreatePolicyPort, CreatePolicyUseCasePort, PolicyValidator,
};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// Use case for creating IAM policies
///
/// This use case orchestrates the policy creation process:
/// 1. Validates the Cedar policy syntax and semantics
/// 2. Persists the policy if validation succeeds
/// 3. Returns a view of the created policy
///
/// # Architecture Note
///
/// This struct uses trait objects (Arc<dyn Trait>) instead of generics for simplicity.
/// This is the idiomatic Rust approach for dependency injection without frameworks.
pub struct CreatePolicyUseCase {
    /// Port for persisting policies (only create operation)
    policy_port: Arc<dyn CreatePolicyPort>,

    /// Port for validating Cedar policy content
    validator: Arc<dyn PolicyValidator>,
}

impl CreatePolicyUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_port` - Implementation of `CreatePolicyPort` for persistence
    /// * `validator` - Implementation of `PolicyValidator` for validation
    pub fn new(
        policy_port: Arc<dyn CreatePolicyPort>,
        validator: Arc<dyn PolicyValidator>,
    ) -> Self {
        Self {
            policy_port,
            validator,
        }
    }

    /// Execute the create policy use case (internal implementation)
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy details
    ///
    /// # Returns
    ///
    /// On success, returns `Ok(PolicyView)` with the created policy information.
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::EmptyPolicyContent` - Policy content is empty
    /// - `CreatePolicyError::InvalidPolicyContent` - Policy fails Cedar validation
    /// - `CreatePolicyError::PolicyAlreadyExists` - Policy ID already in use
    /// - `CreatePolicyError::RepositoryError` - Database or storage failure
    #[instrument(skip(self, command), fields(policy_id = %command.policy_id))]
    async fn execute_impl(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<PolicyView, CreatePolicyError> {
        info!("Creating policy with id: {}", command.policy_id);

        // Validate input
        if command.policy_content.trim().is_empty() {
            warn!("Policy creation failed: empty content");
            return Err(CreatePolicyError::EmptyPolicyContent);
        }

        // Validate policy syntax using hodei-policies
        info!("Validating policy content");
        let validation_command =
            hodei_policies::features::validate_policy::dto::ValidatePolicyCommand {
                content: command.policy_content.clone(),
            };

        let validation_result = self
            .validator
            .validate(validation_command)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed(e.to_string()))?;

        if !validation_result.is_valid || !validation_result.errors.is_empty() {
            warn!(
                "Policy validation failed with {} errors",
                validation_result.errors.len()
            );
            let error_messages = validation_result.errors.join(", ");
            return Err(CreatePolicyError::InvalidPolicyContent(error_messages));
        }

        info!("Policy validation successful, persisting policy");

        // Create the policy through the port
        let policy = self.policy_port.create(command).await?;

        info!("Policy created successfully: {}", policy.id());

        // Convert to view DTO
        let now = chrono::Utc::now();

        // Build HRN from policy ID
        let policy_hrn = kernel::Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(), // TODO: Get from context
            "Policy".to_string(),
            policy.id().to_string(),
        );

        let view = PolicyView {
            id: policy_hrn,
            content: policy.content().to_string(),
            description: None, // Policy doesn't have description in kernel
            created_at: now,
            updated_at: now,
        };

        Ok(view)
    }
}

// Implement CreatePolicyUseCasePort trait for the use case
#[async_trait]
impl CreatePolicyUseCasePort for CreatePolicyUseCase {
    async fn execute(&self, command: CreatePolicyCommand) -> Result<PolicyView, CreatePolicyError> {
        self.execute_impl(command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::mocks::{MockCreatePolicyPort, MockPolicyValidator};

    #[tokio::test]
    async fn test_create_policy_success() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_policy_empty_content() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "   ".to_string(),
            description: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(CreatePolicyError::EmptyPolicyContent)));
    }

    #[tokio::test]
    async fn test_create_policy_validation_failure() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::with_errors(vec![
            "Syntax error".to_string(),
        ]));
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "invalid policy".to_string(),
            description: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(CreatePolicyError::InvalidPolicyContent(_))
        ));
    }
}
