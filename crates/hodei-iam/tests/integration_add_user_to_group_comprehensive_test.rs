/// Comprehensive integration tests for add_user_to_group feature

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
async fn test_add_multiple_users_to_same_group() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple users and add them to the group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let users = vec!["Alice", "Bob", "Charlie"];

    for user_name in users {
        let user_view = create_user_uc.execute(CreateUserCommand {
            name: user_name.to_string(),
            email: format!("{}@test.com", user_name.to_lowercase()),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn,
            group_hrn: group_view.hrn.clone(),
        }).await;

        assert!(result.is_ok());
    }

    // Verify all users are in the group
    let all_users = user_repo.find_all().await.unwrap();
    for user in all_users {
        assert_eq!(user.groups().len(), 1);
        assert_eq!(user.groups()[0].to_string(), group_view.hrn);
    }
}

#[tokio::test]
async fn test_add_user_to_multiple_groups() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple groups
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let groups = vec!["developers", "designers", "managers"];

    for group_name in groups {
        let group_view = create_group_uc.execute(CreateGroupCommand {
            group_name: group_name.to_string(),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn,
        }).await;

        assert!(result.is_ok());
    }

    // Verify user is in all groups
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 3);
}

#[tokio::test]
async fn test_complex_user_group_relationships() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    // Create groups
    let dev_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    let ops_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "operations".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create users
    let alice = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let bob = create_user_uc.execute(CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Alice is in both groups
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn.clone(),
        group_hrn: dev_group.hrn.clone(),
    }).await.unwrap();

    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn,
        group_hrn: ops_group.hrn.clone(),
    }).await.unwrap();

    // Bob is only in developers
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: bob.hrn,
        group_hrn: dev_group.hrn,
    }).await.unwrap();

    // Verify relationships
    let all_users = user_repo.find_all().await.unwrap();
    let alice_user = all_users.iter().find(|u| u.name == "Alice").unwrap();
    let bob_user = all_users.iter().find(|u| u.name == "Bob").unwrap();

    assert_eq!(alice_user.groups().len(), 2);
    assert_eq!(bob_user.groups().len(), 1);
}

