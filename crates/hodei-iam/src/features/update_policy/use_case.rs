//! Use case for updating IAM policies
//!
//! This module implements the business logic for updating existing IAM policies.
//! Following Clean Architecture and Vertical Slice Architecture (VSA) principles,
//! this use case is self-contained and depends only on abstract ports.
//!
//! # Flow
//!
//! 1. Receive `UpdatePolicyCommand` from the caller
//! 2. Validate that at least one field is being updated
//! 3. If policy content is provided, validate it via `PolicyValidator`
//! 4. Update the policy through `UpdatePolicyPort`
//! 5. Return updated policy view or appropriate error
//!
//! # Dependencies
//!
//! - `PolicyValidator`: Validates Cedar policy syntax (if content is updated)
//! - `UpdatePolicyPort`: Abstract port for policy persistence (ISP - only update)

use crate::features::update_policy::dto::{PolicyView, UpdatePolicyCommand};
use crate::features::update_policy::error::UpdatePolicyError;
use crate::features::update_policy::ports::{PolicyValidator, UpdatePolicyPort};
use async_trait::async_trait;
use hodei_policies::features::validate_policy::dto::ValidatePolicyCommand;
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// Use case for updating IAM policies
///
/// This use case orchestrates the policy update process:
/// 1. Validates the update command
/// 2. Optionally validates new policy content
/// 3. Updates the policy through the port
/// 4. Returns success or appropriate error
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::{UpdatePolicyUseCase, UpdatePolicyCommand};
/// use std::sync::Arc;
///
/// let validator = Arc::new(CedarPolicyValidator::new());
/// let updater = Arc::new(SurrealPolicyAdapter::new(db));
/// let use_case = UpdatePolicyUseCase::new(validator, updater);
///
/// let command = UpdatePolicyCommand {
///     policy_id: "allow-read-docs".to_string(),
///     policy_content: Some("permit(principal, action, resource);".to_string()),
///     description: Some("Updated description".to_string()),
/// };
///
/// match use_case.execute(command).await {
///     Ok(policy) => println!("Policy updated: {}", policy.hrn),
///     Err(e) => eprintln!("Update failed: {}", e),
/// }
/// ```
pub struct UpdatePolicyUseCase {
    /// Validator for checking Cedar policy syntax
    validator: Arc<dyn PolicyValidator>,

    /// Port for updating policies (only update operation)
    policy_port: Arc<dyn UpdatePolicyPort>,
}

impl UpdatePolicyUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `validator` - Implementation of `PolicyValidator` for syntax validation
    /// * `policy_port` - Implementation of `UpdatePolicyPort` for persistence
    ///
    /// # Arguments
    ///
    /// * `validator` - Implementation of `PolicyValidator` for syntax validation
    /// * `policy_port` - Implementation of `UpdatePolicyPort` for persistence
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let use_case = UpdatePolicyUseCase::new(
    ///     Arc::new(validator),
    ///     Arc::new(policy_port)
    /// );
    /// ```
    pub fn new(
        validator: Arc<dyn PolicyValidator>,
        policy_port: Arc<dyn UpdatePolicyPort>,
    ) -> Self {
        Self {
            validator,
            policy_port,
        }
    }

    /// Execute the update policy use case
    ///
    /// This is the main entry point for updating an IAM policy.
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy ID and optional new content/description
    ///
    /// # Returns
    ///
    /// On success, returns `Ok(PolicyView)` with the updated policy information.
    ///
    /// # Errors
    ///
    /// - `UpdatePolicyError::InvalidPolicyId` - Policy ID is invalid or empty
    /// - `UpdatePolicyError::NoUpdatesProvided` - No fields to update provided
    /// - `UpdatePolicyError::EmptyPolicyContent` - Policy content provided but empty
    /// - `UpdatePolicyError::InvalidPolicyContent` - Policy syntax is invalid
    /// - `UpdatePolicyError::PolicyNotFound` - Policy does not exist
    /// - `UpdatePolicyError::StorageError` - Database or storage failure
    /// - `UpdatePolicyError::SystemPolicyProtected` - Cannot update system policy
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = UpdatePolicyCommand::update_content(
    ///     "my-policy",
    ///     "permit(principal, action, resource);"
    /// );
    ///
    /// let result = use_case.execute(command).await?;
    /// println!("Updated policy: {}", result.hrn);
    /// ```
    #[instrument(skip(self, command), fields(policy_id = %command.policy_id))]
    pub async fn execute(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<PolicyView, UpdatePolicyError> {
        info!("Updating policy: {}", command.policy_id);

        // Validate policy ID
        if command.policy_id.is_empty() {
            warn!("Update failed: policy ID is empty");
            return Err(UpdatePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        // Validate that at least one field is being updated
        if command.policy_content.is_none() && command.description.is_none() {
            warn!("Update failed: no fields to update");
            return Err(UpdatePolicyError::NoUpdatesProvided);
        }

        // Validate policy content if provided
        if let Some(ref content) = command.policy_content {
            if content.trim().is_empty() {
                warn!("Update failed: policy content is empty");
                return Err(UpdatePolicyError::EmptyPolicyContent);
            }

            info!("Validating new policy content");
            let validation_command = ValidatePolicyCommand {
                content: content.clone(),
            };
            let validation_result = self
                .validator
                .validate(validation_command)
                .await
                .map_err(|e| UpdatePolicyError::ValidationFailed(e.to_string()))?;

            if !validation_result.is_valid || !validation_result.errors.is_empty() {
                warn!(
                    "Policy validation failed with {} errors",
                    validation_result.errors.len()
                );
                let error_messages = validation_result.errors.join(", ");
                return Err(UpdatePolicyError::InvalidPolicyContent(error_messages));
            }

            // Note: ValidationResult from hodei-policies doesn't include warnings field
        }

        // Update the policy through the port
        info!("Persisting policy update");
        let updated_view = self.policy_port.update(command).await?;

        info!("Policy updated successfully: {}", updated_view.name);

        Ok(updated_view)
    }
}

// Implement UpdatePolicyPort trait for the use case to enable trait object usage
#[async_trait]
impl UpdatePolicyPort for UpdatePolicyUseCase {
    async fn update(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
        self.execute(command).await
    }
}
