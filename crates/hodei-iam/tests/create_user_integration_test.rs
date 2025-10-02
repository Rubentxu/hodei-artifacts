/// Integration tests for create_user feature
///
/// These tests use in-memory repositories to validate the complete vertical slice

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;

#[tokio::test]
async fn test_create_user_success() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "Alice");
    assert_eq!(view.email, "alice@example.com");
    assert_eq!(view.groups.len(), 0); // No groups initially
    assert_eq!(view.tags.len(), 1);
    assert!(view.hrn.contains("User"));

    // Verify persistence
    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "Alice");
    assert_eq!(users[0].email, "alice@example.com");
}

#[tokio::test]
async fn test_create_multiple_users() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    // Act - Create multiple users
    let cmd1 = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };
    let cmd2 = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec!["developer".to_string()],
    };

    let result1 = use_case.execute(cmd1).await;
    let result2 = use_case.execute(cmd2).await;

    // Assert
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 2);
}
