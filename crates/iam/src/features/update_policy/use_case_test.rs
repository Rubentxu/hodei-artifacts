#[cfg(test)]
mod tests {
    use crate::domain::policy::Policy;
    use crate::domain::validation::ValidationResult;
    use crate::features::update_policy::dto::UpdatePolicyCommand;
    use crate::features::update_policy::ports::{PolicyUpdater, PolicyUpdateValidator, PolicyUpdateEventPublisher};
    use crate::features::update_policy::use_case::UpdatePolicyUseCase;
    use crate::infrastructure::errors::IamError;
    use crate::test_utils::{create_test_policy, policy_id};
    use async_trait::async_trait;
    use mockall::mock;
    use mockall::predicate::*;
    use shared::hrn::PolicyId;
    use std::sync::Arc;

    // Create mocks for the traits
    mock! {
        PolicyUpdaterImpl {}
        #[async_trait]
        impl PolicyUpdater for PolicyUpdaterImpl {
            async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
            async fn update(&self, policy: Policy) -> Result<Policy, IamError>;
            async fn exists(&self, id: &PolicyId) -> Result<bool, IamError>;
        }
    }

    mock! {
        Validator {}
        #[async_trait]
        impl PolicyUpdateValidator for Validator {
            async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError>;
        }
    }

    mock! {
        EventPublisher {}
        #[async_trait]
        impl PolicyUpdateEventPublisher for EventPublisher {
            async fn publish_policy_updated(&self, old_policy: &Policy, new_policy: &Policy) -> Result<(), IamError>;
        }
    }

    #[tokio::test]
    async fn test_update_policy_success() {
        // Arrange
        let mut mock_updater = MockPolicyUpdaterImpl::new();
        let mut mock_validator = MockValidator::new();
        let mut mock_publisher = MockEventPublisher::new();
        
        let policy_id = policy_id();
        let mut policy = create_test_policy();
        policy.id = policy_id.clone();

        mock_updater
            .expect_get_by_id()
            .with(eq(policy_id.clone()))
            .times(1)
            .returning(move |_| Ok(Some(policy.clone())));

        mock_validator
            .expect_validate_syntax()
            .times(1)
            .returning(|_| Ok(ValidationResult::valid()));

        mock_updater
            .expect_update()
            .times(1)
            .returning(|policy| Ok(policy));

        mock_publisher
            .expect_publish_policy_updated()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_updater),
            Arc::new(mock_validator),
            Arc::new(mock_publisher),
        );

        let command = UpdatePolicyCommand {
            id: policy_id.clone(),
            name: Some("Updated Policy".to_string()),
            description: Some("Updated description".to_string()),
            content: Some("permit(principal, action, resource);".to_string()),
            tags: Some(vec!["updated".to_string()]),
            updated_by: "test-user".to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.id, policy_id);
    }

    #[tokio::test]
    async fn test_update_policy_not_found() {
        // Arrange
        let mut mock_updater = MockPolicyUpdaterImpl::new();
        let mock_validator = MockValidator::new();
        let mock_publisher = MockEventPublisher::new();
        
        let policy_id = policy_id();

        mock_updater
            .expect_get_by_id()
            .with(eq(policy_id.clone()))
            .times(1)
            .returning(|_| Ok(None));

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_updater),
            Arc::new(mock_validator),
            Arc::new(mock_publisher),
        );

        let command = UpdatePolicyCommand {
            id: policy_id.clone(),
            name: Some("Updated Policy".to_string()),
            description: Some("Updated description".to_string()),
            content: Some("permit(principal, action, resource);".to_string()),
            tags: Some(vec!["updated".to_string()]),
            updated_by: "test-user".to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyNotFound(id) => assert_eq!(id, policy_id),
            _ => panic!("Expected PolicyNotFound error"),
        }
    }
}