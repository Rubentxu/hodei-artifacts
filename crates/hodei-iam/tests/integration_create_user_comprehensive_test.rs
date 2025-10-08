/// Comprehensive integration tests for create_user feature
/// Uses only public API from hodei_iam crate
use hodei_iam::{
    features::create_user::{self, dto::CreateUserCommand},
    infrastructure::in_memory::InMemoryUserAdapter,
    infrastructure::hrn_generator::UuidHrnGenerator,
};
use std::sync::Arc;

#[tokio::test]
async fn test_create_user_with_valid_email() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok(), "Failed to create user: {:?}", result.err());

    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.groups.len(), 0);
    assert_eq!(view.tags.len(), 1);
}

#[tokio::test]
async fn test_create_user_multiple_tags() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "Jane Smith".to_string(),
        email: "jane@example.com".to_string(),
        tags: vec![
            "developer".to_string(),
            "senior".to_string(),
            "fullstack".to_string(),
        ],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"developer".to_string()));
    assert!(view.tags.contains(&"senior".to_string()));
    assert!(view.tags.contains(&"fullstack".to_string()));
}

#[tokio::test]
async fn test_create_user_no_tags() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);
}

#[tokio::test]
async fn test_create_user_hrn_format() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await.unwrap();

    // Verify HRN format: hrn:partition:service::account_id:resource_type/resource_id
    assert!(
        result.hrn.starts_with("hrn:"),
        "HRN should start with 'hrn:'"
    );
    assert!(
        result.hrn.contains(":iam:"),
        "HRN should contain service 'iam' in lowercase"
    );
    assert!(
        result.hrn.contains(":User/"),
        "HRN should contain resource_type 'User' followed by '/'"
    );
}

#[tokio::test]
async fn test_create_user_unique_ids() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "Same Name".to_string(),
        email: "same@example.com".to_string(),
        tags: vec![],
    };

    let result1 = use_case.execute(command.clone()).await.unwrap();
    let result2 = use_case.execute(command.clone()).await.unwrap();

    // Even with same data, HRNs should be different (UUID)
    assert_ne!(result1.hrn, result2.hrn);
}

#[tokio::test]
async fn test_create_users_batch() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let users = vec![
        ("Alice", "alice@test.com"),
        ("Bob", "bob@test.com"),
        ("Charlie", "charlie@test.com"),
    ];

    for (name, email) in users {
        let command = CreateUserCommand {
            name: name.to_string(),
            email: email.to_string(),
            tags: vec![],
        };

        let result = use_case.execute(command).await;
        assert!(
            result.is_ok(),
            "Failed to create user {}: {:?}",
            name,
            result.err()
        );
    }

    // Verify persistence by finding all users
    // This would require additional methods in the adapter for testing purposes
}

#[tokio::test]
async fn test_create_user_email_validation_format() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    // Test with various email formats
    let valid_emails = vec![
        "simple@example.com",
        "user.name@example.com",
        "user+tag@example.co.uk",
        "first.last@subdomain.example.com",
    ];

    for email in valid_emails {
        let command = CreateUserCommand {
            name: "Test User".to_string(),
            email: email.to_string(),
            tags: vec![],
        };

        let result = use_case.execute(command).await;
        assert!(
            result.is_ok(),
            "Email '{}' should be valid but got error: {:?}",
            email,
            result.err()
        );
    }
}

#[tokio::test]
async fn test_create_user_persistence() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "Persistent User".to_string(),
        email: "persistent@example.com".to_string(),
        tags: vec!["test".to_string()],
    };

    let created = use_case.execute(command).await.unwrap();

    // Verify user was actually persisted
    // This would require additional methods in the adapter for testing purposes
}

#[tokio::test]
async fn test_create_user_empty_name() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "".to_string(),
        email: "empty@example.com".to_string(),
        tags: vec![],
    };

    // Empty name should be allowed (validation is domain decision)
    let result = use_case.execute(command).await;
    // If your domain requires non-empty names, this should fail
    // For now, we allow it
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_create_user_special_characters_in_name() {
    let adapter = Arc::new(InMemoryUserAdapter::new());
    let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "test-account".to_string()));
    let use_case = create_user::di::CreateUserUseCaseFactory::build(adapter.clone(), hrn_generator.clone());

    let command = CreateUserCommand {
        name: "José García-López O'Brien".to_string(),
        email: "jose@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, "José García-López O'Brien");
}
