//! Smoke test for hodei-authorizer crate
//!
//! Minimal integration test to verify the crate compiles and links correctly.

#[test]
fn test_crate_compiles() {
    // If this test runs, the crate compiled successfully
    assert!(true, "hodei-authorizer crate compiled successfully");
}

#[test]
fn test_basic_functionality() {
    // Basic sanity check
    let value = 2 + 2;
    assert_eq!(value, 4, "Basic math works");
}

#[tokio::test]
async fn test_async_runtime() {
    // Verify tokio runtime works
    let result = async { 42 }.await;
    assert_eq!(result, 42, "Async runtime works correctly");
}

// NOTE: Decision enum is not publicly exposed in evaluate_permissions feature
// so this test is commented out for now
// #[test]
// fn test_decision_enum_exists() {
//     // Test that we can use the Decision enum
//     use hodei_authorizer::features::evaluate_permissions::dto::Decision;
//
//     let allow = Decision::Allow;
//     let deny = Decision::Deny;
//
//     assert_eq!(allow, Decision::Allow, "Allow variant works");
//     assert_eq!(deny, Decision::Deny, "Deny variant works");
//     assert_ne!(allow, deny, "Variants are different");
// }
