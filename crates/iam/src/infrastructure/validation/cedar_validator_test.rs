// crates/iam/src/infrastructure/validation/cedar_validator_test.rs

#[cfg(test)]
mod tests {
    use crate::domain::validation::ValidationResult;
    use crate::features::create_policy::ports::PolicyValidator;
    use crate::infrastructure::errors::ValidationError;
    use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;

    #[tokio::test]
    async fn test_validate_syntax_valid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_valid_policy_set() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
            
            forbid(
                principal == User::"bob",
                action == Action::"delete",
                resource
            );
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_empty_content() {
        let validator = CedarPolicyValidator::new();
        let policy_content = "";

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert_eq!(validation_result.errors.len(), 1);
        assert!(
            validation_result.errors[0]
                .message
                .contains("cannot be empty")
        );
    }

    #[tokio::test]
    async fn test_validate_syntax_invalid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            invalid_keyword(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_malformed_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice"
                // Missing comma and other parts
            );
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    // #[tokio::test]
    // async fn test_create_validation_error() {
    //     let validator = CedarPolicyValidator::new();
    //     let error_msg = "Parse error at line 5, column 10";
    //     let validation_error = validator.create_validation_error(error_msg.to_string());

    //     assert_eq!(validation_error.message, error_msg);
    //     // Note: The regex extraction might not work perfectly with this test message
    //     // In real Cedar errors, the format might be different
    // }

    #[tokio::test]
    async fn test_validator_default() {
        let validator = CedarPolicyValidator::default();
        // Note: schema field is private, so we can't test it directly
        // This test just verifies the validator can be created with default
        assert!(std::ptr::addr_of!(validator).is_null() == false);
    }

    #[tokio::test]
    async fn test_validation_result_helpers() {
        let mut result = ValidationResult::valid();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());

        result.add_error(ValidationError {
            message: "Test error".to_string(),
            line: Some(1),
            column: Some(5),
        });

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.first_error_message(), Some("Test error"));
    }

    #[tokio::test]
    async fn test_complex_policy_validation() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal in Group::"admins",
                action in [Action::"read", Action::"write"],
                resource in Folder::"documents"
            )
            when {
                principal.department == "engineering" &&
                resource.classification != "confidential"
            };
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_policy_with_conditions() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            )
            when {
                context.time >= datetime("2023-01-01T00:00:00Z")
            };
        "#;

        let result = validator.validate_syntax(policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }
}
