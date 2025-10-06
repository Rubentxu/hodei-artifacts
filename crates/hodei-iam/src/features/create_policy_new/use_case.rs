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

use crate::features::create_policy_new::dto::{CreatePolicyCommand, PolicyView};
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::{CreatePolicyPort, PolicyValidator};
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// Use case for creating IAM policies
///
/// This use case orchestrates the policy creation process:
/// 1. Validates the Cedar policy syntax and semantics
/// 2. Persists the policy if validation succeeds
/// 3. Returns a view of the created policy
///
/// # Type Parameters
///
/// - `P`: Implementation of `CreatePolicyPort` for persistence
/// - `V`: Implementation of `PolicyValidator` for validation
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::{CreatePolicyUseCase, CreatePolicyCommand};
/// use std::sync::Arc;
///
/// let validator = Arc::new(CedarValidator::new());
/// let persister = Arc::new(SurrealPolicyAdapter::new(db));
/// let use_case = CreatePolicyUseCase::new(persister, validator);
///
/// let command = CreatePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
///     policy_content: "permit(principal, action, resource);".to_string(),
///     description: Some("Allow document reading".to_string()),
/// };
///
/// match use_case.execute(command).await {
///     Ok(view) => println!("Policy created: {}", view.id),
///     Err(e) => eprintln!("Creation failed: {}", e),
/// }
/// ```
pub struct CreatePolicyUseCase<P, V>
where
    P: CreatePolicyPort,
    V: PolicyValidator,
{
    /// Port for persisting policies (only create operation)
    policy_port: Arc<P>,

    /// Port for validating Cedar policy content
    validator: Arc<V>,
}

impl<P, V> CreatePolicyUseCase<P, V>
where
    P: CreatePolicyPort,
    V: PolicyValidator,
{
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_port` - Implementation of `CreatePolicyPort` for persistence
    /// * `validator` - Implementation of `PolicyValidator` for validation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let use_case = CreatePolicyUseCase::new(
    ///     Arc::new(policy_port),
    ///     Arc::new(validator),
    /// );
    /// ```
    pub fn new(policy_port: Arc<P>, validator: Arc<V>) -> Self {
        Self {
            policy_port,
            validator,
        }
    }

    /// Execute the create policy use case
    ///
    /// This is the main entry point for creating a new IAM policy.
    /// It performs validation and persistence in a single transaction.
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy details (id, content, description)
    ///
    /// # Returns
    ///
    /// On success, returns a `PolicyView` DTO with the created policy details
    /// including generated metadata (timestamps, HRN).
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::EmptyPolicyContent` - Policy content is empty
    /// - `CreatePolicyError::InvalidPolicyId` - Policy ID is invalid or empty
    /// - `CreatePolicyError::ValidationFailed` - Validation service error
    /// - `CreatePolicyError::InvalidPolicyContent` - Policy syntax/semantic errors
    /// - `CreatePolicyError::StorageError` - Database or storage failure
    /// - `CreatePolicyError::PolicyAlreadyExists` - Policy ID already in use
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = CreatePolicyCommand {
    ///     policy_id: "my-policy".to_string(),
    ///     policy_content: "permit(principal, action, resource);".to_string(),
    ///     description: Some("My policy".to_string()),
    /// };
    ///
    /// let view = use_case.execute(command).await?;
    /// assert_eq!(view.content, "permit(principal, action, resource);");
    /// ```
    #[instrument(skip(self, command), fields(policy_id = %command.policy_id))]
    pub async fn execute(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<PolicyView, CreatePolicyError> {
        info!("Creating policy with id: {}", command.policy_id);

        // Validate input
        if command.policy_id.is_empty() {
            warn!("Policy creation failed: empty policy ID");
            return Err(CreatePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        if command.policy_content.trim().is_empty() {
            warn!("Policy creation failed: empty policy content");
            return Err(CreatePolicyError::EmptyPolicyContent);
        }

        // 1. Validate policy content through port
        info!("Validating policy content");
        let validation_result = self
            .validator
            .validate_policy(&command.policy_content)
            .await
            .map_err(|e| {
                warn!("Policy validation service error: {}", e);
                CreatePolicyError::ValidationFailed(e.to_string())
            })?;

        // Check if validation found errors
        if !validation_result.is_valid {
            let error_messages: Vec<String> = validation_result
                .errors
                .iter()
                .map(|e| {
                    if let (Some(line), Some(col)) = (e.line, e.column) {
                        format!("{} (line {}, column {})", e.message, line, col)
                    } else {
                        e.message.clone()
                    }
                })
                .collect();

            let error_summary = error_messages.join("; ");
            warn!("Policy validation failed: {}", error_summary);
            return Err(CreatePolicyError::InvalidPolicyContent(error_summary));
        }

        // Log warnings if any
        if !validation_result.warnings.is_empty() {
            for warning in &validation_result.warnings {
                warn!(
                    "Policy validation warning ({}): {}",
                    warning.severity, warning.message
                );
            }
        }

        info!("Policy validation successful");

        // 2. Create policy through port
        info!("Persisting policy");
        let policy = self.policy_port.create(command).await.map_err(|e| {
            warn!("Policy persistence failed: {}", e);
            e
        })?;

        info!("Policy created successfully with HRN: {}", policy.id);

        // 3. Return DTO
        Ok(PolicyView {
            id: policy.id,
            content: policy.content,
            description: policy.description,
            created_at: policy.created_at,
            updated_at: policy.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Tests use mocks from mocks.rs - see use_case_test.rs for full test suite
}
