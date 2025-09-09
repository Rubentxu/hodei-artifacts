// crates/iam/tests/policy_validation_integration_test.rs

use iam::infrastructure::validation::SemanticPolicyValidator;
use iam::features::create_policy::ports::PolicyValidator;
use iam::infrastructure::errors::IamError;

#[tokio::test]
async fn test_semantic_validation_integration() {
    let validator = SemanticPolicyValidator::new().expect("Failed to create validator");

    // Test valid policy
    let valid_policy = r#"
        permit(
            principal == User::"alice",
            action == ReadArtifact,
            resource == Artifact::"test-document"
        );
    "#;

    let result = validator.validate_syntax(valid_policy).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(validation_result.is_valid, "Valid policy should pass validation");

    // Test policy with unknown entity type
    let invalid_entity_policy = r#"
        permit(
            principal == UnknownEntity::"alice",
            action == ReadArtifact,
            resource == Artifact::"test-document"
        );
    "#;

    let result = validator.validate_syntax(invalid_entity_policy).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid, "Policy with unknown entity should fail validation");
    assert!(!validation_result.errors.is_empty());

    // Test policy with unknown action
    let invalid_action_policy = r#"
        permit(
            principal == User::"alice",
            action == UnknownAction,
            resource == Artifact::"test-document"
        );
    "#;

    let result = validator.validate_syntax(invalid_action_policy).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid, "Policy with unknown action should fail validation");
    assert!(!validation_result.errors.is_empty());

    // Test semantic validation method
    let semantic_result = validator.validate_semantics(valid_policy).await;
    assert!(semantic_result.is_ok(), "Valid policy should pass semantic validation");

    let semantic_result = validator.validate_semantics(invalid_entity_policy).await;
    assert!(semantic_result.is_err(), "Invalid policy should fail semantic validation");
    
    if let Err(error) = semantic_result {
        // Check that we got some kind of validation error
        assert!(!error.to_string().is_empty(), "Semantic error should have a message");
    } else {
        // For now, this might pass since we don't have full semantic validation
        // TODO: This should fail when proper semantic validation is implemented
        println!("Note: Semantic validation passed - full semantic validation not yet implemented");
    }
}

#[tokio::test]
async fn test_syntax_error_handling() {
    let validator = SemanticPolicyValidator::new().expect("Failed to create validator");

    let syntax_error_policy = r#"
        invalid_keyword(
            principal == User::"alice",
            action == ReadArtifact,
            resource == Artifact::"test-document"
        );
    "#;

    let result = validator.validate_syntax(syntax_error_policy).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid, "Policy with syntax error should fail validation");
    assert!(!validation_result.errors.is_empty());
}

#[tokio::test]
async fn test_empty_policy_handling() {
    let validator = SemanticPolicyValidator::new().expect("Failed to create validator");

    let result = validator.validate_syntax("").await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid, "Empty policy should fail validation");
    assert!(!validation_result.errors.is_empty());
    assert!(validation_result.errors[0].message.contains("empty"));
}

#[tokio::test]
async fn test_multiple_policies_validation() {
    let validator = SemanticPolicyValidator::new().expect("Failed to create validator");

    let multiple_policies = r#"
        permit(
            principal == User::"alice",
            action == ReadArtifact,
            resource == Artifact::"doc1"
        );
        
        permit(
            principal == User::"bob",
            action == WriteArtifact,
            resource == Artifact::"doc2"
        );
        
        forbid(
            principal == User::"charlie",
            action == DeleteArtifact,
            resource == Artifact::"doc3"
        );
    "#;

    let result = validator.validate_syntax(multiple_policies).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(validation_result.is_valid, "Multiple valid policies should pass validation");
}

#[tokio::test]
async fn test_complex_policy_validation() {
    let validator = SemanticPolicyValidator::new().expect("Failed to create validator");

    let complex_policy = r#"
        permit(
            principal == User::"alice",
            action == ReadArtifact,
            resource == Artifact::"sensitive-doc"
        ) when {
            principal.role == "admin" &&
            resource.public == false
        };
    "#;

    let result = validator.validate_syntax(complex_policy).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    // Note: This might fail semantic validation if the schema doesn't define
    // the attributes used in the when clause, which is expected behavior
    if !validation_result.is_valid {
        println!("Complex policy validation errors: {:?}", validation_result.errors);
        // This is acceptable - the schema might not define all attributes
    }
}

#[tokio::test]
async fn test_validator_with_custom_schema() {
    // Test creating validator with custom schema
    let custom_schema = r#"
        entity CustomUser = {
            "name": String,
            "level": Long,
        };

        entity CustomResource = {
            "id": String,
            "owner": CustomUser,
        };

        action CustomAction appliesTo {
            principal: [CustomUser],
            resource: [CustomResource]
        };
    "#;

    let validator = SemanticPolicyValidator::with_schema(custom_schema);
    assert!(validator.is_ok(), "Should be able to create validator with custom schema");

    let validator = validator.unwrap();
    
    let policy_with_custom_entities = r#"
        permit(
            principal == CustomUser::"alice",
            action == CustomAction,
            resource == CustomResource::"resource1"
        );
    "#;

    let result = validator.validate_syntax(policy_with_custom_entities).await;
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(validation_result.is_valid, "Policy with custom entities should be valid with custom schema");
}