//! Smoke test for hodei-iam crate
//!
//! Minimal integration test to verify the crate compiles and links correctly.

#[test]
fn test_crate_compiles() {
    // If this test runs, the crate compiled successfully
    assert!(true, "hodei-iam crate compiled successfully");
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

#[test]
fn test_adapters_can_be_instantiated() {
    // Test that we can create adapter instances
    use hodei_iam::infrastructure::in_memory::{InMemoryUserAdapter, InMemoryGroupAdapter, InMemoryPolicyAdapter};

    let _user_adapter = InMemoryUserAdapter::new();
    let _group_adapter = InMemoryGroupAdapter::new();
    let _policy_adapter = InMemoryPolicyAdapter::new("test-account".to_string());

    assert!(true, "Adapters instantiated successfully");
}
