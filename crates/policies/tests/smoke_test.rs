//! Smoke test for policies crate
//!
//! This simple integration test verifies that the crate compiles
//! and basic functionality works.

#[test]
fn test_crate_compiles() {
    // If this test runs, the crate compiled successfully
    assert!(true);
}

#[test]
fn test_crate_can_be_imported() {
    // Verify the crate can be imported
    use policies;

    let _ = stringify!(policies);
    assert!(true, "Crate can be imported");
}

#[test]
fn test_infrastructure_module_accessible() {
    // Verify infrastructure module is accessible
    use policies::infrastructure;

    let _ = stringify!(infrastructure);
    assert!(true, "Infrastructure module is accessible");
}

#[test]
fn test_validator_module_accessible() {
    // Verify validator module is accessible
    use policies::infrastructure::validator;

    let _ = stringify!(validator);
    assert!(true, "Validator module is accessible");
}

#[test]
fn test_validator_dto_accessible() {
    // Verify validator DTOs are accessible
    use policies::infrastructure::validator::dto;

    let _ = stringify!(dto);
    assert!(true, "Validator DTOs are accessible");
}

#[test]
fn test_validate_policy_query_creation() {
    // Test that ValidatePolicyQuery can be created
    use policies::infrastructure::validator::dto::ValidatePolicyQuery;

    let query = ValidatePolicyQuery::new("permit(principal, action, resource);".to_string());

    assert_eq!(query.policy_content, "permit(principal, action, resource);");
}

#[tokio::test]
async fn test_async_runtime_works() {
    // Verify async runtime works
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    assert!(true, "Async runtime works");
}

#[tokio::test]
async fn test_validator_use_case_can_be_created() {
    // Test that ValidatePolicyUseCase can be instantiated
    use policies::infrastructure::validator::use_case::ValidatePolicyUseCase;

    let _use_case = ValidatePolicyUseCase::new();

    assert!(true, "ValidatePolicyUseCase can be created");
}
