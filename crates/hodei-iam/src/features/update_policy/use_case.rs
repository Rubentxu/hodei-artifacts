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
/// # Type Parameters
///
/// - `V`: Implementation of `PolicyValidator` for syntax validation
/// - `P`: Implementation of `UpdatePolicyPort` for persistence
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
pub struct UpdatePolicyUseCase<V, P>
where
    V: PolicyValidator,
    P: UpdatePolicyPort,
{
    /// Validator for checking Cedar policy syntax
    validator: Arc<V>,

    /// Port for updating policies (only update operation)
    policy_port: Arc<P>,
}

impl<V, P> UpdatePolicyUseCase<V, P>
where
    V: PolicyValidator,
    P: UpdatePolicyPort,
{
    /// Create a new instance of the use case
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
    pub fn new(validator: Arc<V>, policy_port: Arc<P>) -> Self {
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
    pub async fn execute(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
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
            let validation_result = self.validator.validate_policy(content).await
                .map_err(|e| UpdatePolicyError::ValidationFailed(e.to_string()))?;

            if !validation_result.is_valid || !validation_result.errors.is_empty() {
                warn!("Policy validation failed: {:?}", validation_result.errors);
                let error_messages: Vec<String> = validation_result
                    .errors
                    .iter()
                    .map(|e| e.message.clone())
                    .collect();
                return Err(UpdatePolicyError::InvalidPolicyContent(
                    error_messages.join("; "),
                ));
            }

            if !validation_result.warnings.is_empty() {
                info!("Policy has warnings: {:?}", validation_result.warnings);
            }
        }

        // Update the policy through the port
        info!("Persisting policy update");
        let updated_view = self.policy_port.update(command).await?;

        info!("Policy updated successfully: {}", updated_view.name);

        Ok(updated_view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::update_policy::mocks::{MockPolicyValidator, MockUpdatePolicyPort};

    #[tokio::test]
    async fn test_update_policy_content_success() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand::update_content(
            "test-policy",
            "permit(principal, action, resource);"
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
    }

    #[tokio::test]
    async fn test_update_policy_description_only() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand::update_description(
            "test-policy",
            "Updated description"
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_policy_empty_id_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand {
            policy_id: "".to_string(),
            policy_content: Some("permit(...);".to_string()),
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::InvalidPolicyId(_) => {}
            e => panic!("Expected InvalidPolicyId, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_update_policy_no_updates_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: None,
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::NoUpdatesProvided => {}
            e => panic!("Expected NoUpdatesProvided, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_update_policy_empty_content_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: Some("   ".to_string()), // Whitespace only
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::EmptyPolicyContent => {}
            e => panic!("Expected EmptyPolicyContent, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_update_policy_invalid_content_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::with_errors(vec![
            "Syntax error: invalid token".to_string()
        ]));
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand::update_content(
            "test-policy",
            "invalid cedar syntax!!!"
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::InvalidPolicyContent(_) => {}
            e => panic!("Expected InvalidPolicyContent, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_update_policy_not_found() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::with_not_found_error());
        let use_case = UpdatePolicyUseCase::new(validator, port);

        let command = UpdatePolicyCommand::update_content(
            "nonexistent",
            "permit(...);"
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            UpdatePolicyError::PolicyNotFound(_) => {}
            e => panic!("Expected PolicyNotFound, got: {:?}", e),
        }
    }
}
