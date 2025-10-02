/// Integration tests for add_user_to_group feature
///
/// These tests use in-memory repositories and coordinate between two aggregates

use hodei_iam::{
    features::{
        create_user::{self as create_user_feature, dto::CreateUserCommand},
        create_group::{self as create_group_feature, dto::CreateGroupCommand},
        add_user_to_group::{self, dto::AddUserToGroupCommand},
    },
    shared::{
        application::ports::{UserRepository, GroupRepository},
        infrastructure::persistence::{InMemoryUserRepository, InMemoryGroupRepository},
    },
};
use std::sync::Arc;

#[tokio::test]
async fn test_add_user_to_group_success() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_cmd = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    };
    let user_view = create_user_uc.execute(user_cmd).await.unwrap();

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_cmd = CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    };
    let group_view = create_group_uc.execute(group_cmd).await.unwrap();

    // Act - Add user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert
    assert!(result.is_ok());

    // Verify that the user now belongs to the group
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 1);
    assert_eq!(users[0].groups()[0].to_string(), group_view.hrn);
}

#[tokio::test]
async fn test_add_user_to_group_idempotent() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user and group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Add user to group twice
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };

    let result1 = add_uc.execute(add_cmd.clone()).await;
    let result2 = add_uc.execute(add_cmd).await;

    // Assert - Both operations succeed
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // Verify that the user only has one group (no duplicates)
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users[0].groups().len(), 1);
}

#[tokio::test]
async fn test_add_user_to_nonexistent_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a user (no group)
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add user to nonexistent group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn,
        group_hrn: "hrn:hodei:iam:default:Group:nonexistent".to_string(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid group HRN") || err_msg.contains("Group not found"));
}

#[tokio::test]
async fn test_add_nonexistent_user_to_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a group (no user)
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add nonexistent user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: "hrn:hodei:iam:default:User:nonexistent".to_string(),
        group_hrn: group_view.hrn,
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid user HRN") || err_msg.contains("User not found"));
}
