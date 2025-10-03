/// Comprehensive integration tests for create_user feature

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_create_user_with_valid_email() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.groups.len(), 0);
    assert_eq!(view.tags.len(), 1);
}

#[tokio::test]
async fn test_create_user_multiple_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Jane Smith".to_string(),
        email: "jane@example.com".to_string(),
        tags: vec!["developer".to_string(), "senior".to_string(), "fullstack".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"developer".to_string()));
    assert!(view.tags.contains(&"senior".to_string()));
}

#[tokio::test]
async fn test_create_user_no_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

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
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await.unwrap();

    // Verify HRN format: hrn:partition:service::account_id:resource_type/resource_id
    assert!(result.hrn.starts_with("hrn:"), "HRN should start with 'hrn:'");
    assert!(result.hrn.contains(":iam:"), "HRN should contain service 'iam' in lowercase");
    assert!(result.hrn.contains(":User/"), "HRN should contain resource_type 'User' followed by '/'");
}

#[tokio::test]
async fn test_create_user_unique_ids() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

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
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

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
        assert!(result.is_ok());
    }

    let all_users = repo.find_all().await.unwrap();
    assert_eq!(all_users.len(), 3);
}
