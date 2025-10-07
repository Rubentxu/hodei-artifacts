use super::dto::ValidatePolicyCommand;
use super::use_case::ValidatePolicyUseCase;

#[tokio::test]
async fn test_valid_policy_returns_is_valid_true() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
    };
    let result = use_case.execute(command).await.unwrap();
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_invalid_policy_returns_is_valid_false_with_errors() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand {
        content: "permit(principal, action);".to_string(),
    }; // Sintaxis incorrecta
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("wrong number of arguments"));
}

#[tokio::test]
async fn test_empty_policy_is_invalid() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand {
        content: "   ".to_string(),
    };
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert_eq!(result.errors[0], "Policy content cannot be empty");
}
