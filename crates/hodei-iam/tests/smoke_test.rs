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
fn test_repositories_can_be_instantiated() {
    // Test that we can create repository instances
    use hodei_iam::infrastructure::InMemoryGroupRepository;
    use hodei_iam::infrastructure::InMemoryUserRepository;

    let _user_repo = InMemoryUserRepository::new();
    let _group_repo = InMemoryGroupRepository::new();

    assert!(true, "Repositories instantiated successfully");
}
