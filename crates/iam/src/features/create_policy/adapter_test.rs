// crates/iam/src/features/create_policy/adapter_test.rs

#[cfg(test)]
mod tests {
    use crate::features::create_policy::adapter::{CedarPolicyValidatorAdapter, SimplePolicyEventPublisherAdapter};
    use crate::features::create_policy::ports::{PolicyValidator, PolicyEventPublisher};
    use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
    use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
    use crate::test_utils::{create_test_policy, PolicyBuilder};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_cedar_policy_validator_adapter() {
        // Arrange
        let validator = Arc::new(CedarPolicyValidator::new());
        let adapter = CedarPolicyValidatorAdapter::new(validator);

        // Test valid policy
        let valid_content = "permit(principal, action, resource);";
        let result = adapter.validate_syntax(valid_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);

        // Test invalid policy
        let invalid_content = "invalid_syntax_here";
        let result = adapter.validate_syntax(invalid_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_simple_policy_event_publisher_adapter() {
        // Arrange
        let publisher = Arc::new(SimplePolicyEventPublisher::new());
        let adapter = SimplePolicyEventPublisherAdapter::new(publisher);
        let test_policy = create_test_policy();

        // Act
        let result = adapter.publish_policy_created(&test_policy).await;

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn test_adapter_creation() {
        // Test that adapters can be created successfully
        let validator = Arc::new(CedarPolicyValidator::new());
        let validator_adapter = CedarPolicyValidatorAdapter::new(validator);
        
        let publisher = Arc::new(SimplePolicyEventPublisher::new());
        let publisher_adapter = SimplePolicyEventPublisherAdapter::new(publisher);

        // Verify adapters were created (compilation test)
        assert!(std::ptr::addr_of!(validator_adapter).is_null() == false);
        assert!(std::ptr::addr_of!(publisher_adapter).is_null() == false);
    }
}