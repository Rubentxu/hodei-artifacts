/// Comprehensive integration tests for add_user_to_group feature
/// Uses only public API from hodei_iam crate
use hodei_iam::{
    features::{
        add_user_to_group::{self, dto::AddUserToGroupCommand},
        create_group::{self, dto::CreateGroupCommand},
        create_user::{self, dto::CreateUserCommand},
    },
    infrastructure::{InMemoryGroupRepository, InMemoryUserRepository},
    ports::UserRepository,
};
use std::sync::Arc;

#[tokio::test]
async fn test_add_user_to_group_basic() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc
        .execute(CreateUserCommand {
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Create a group
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Developers".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Add user to group
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());
    let result = add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn.clone(),
        })
        .await;

    assert!(
        result.is_ok(),
        "Failed to add user to group: {:?}",
        result.err()
    );

    // Verify the user is in the group
    let user = user_repo
        .find_by_hrn(&kernel::Hrn::from_string(&user_view.hrn).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0].to_string(), group_view.hrn);
}

#[tokio::test]
async fn test_add_multiple_users_to_same_group() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a group
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Developers".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Create multiple users and add them to the group
    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());

    let users = vec!["Alice", "Bob", "Charlie"];

    for user_name in users {
        let user_view = create_user_uc
            .execute(CreateUserCommand {
                name: user_name.to_string(),
                email: format!("{}@test.com", user_name.to_lowercase()),
                tags: vec![],
            })
            .await
            .unwrap();

        let result = add_uc
            .execute(AddUserToGroupCommand {
                user_hrn: user_view.hrn,
                group_hrn: group_view.hrn.clone(),
            })
            .await;

        assert!(
            result.is_ok(),
            "Failed to add user {} to group: {:?}",
            user_name,
            result.err()
        );
    }

    // Verify all users are in the group
    let all_users = user_repo.find_all().await.unwrap();
    assert_eq!(all_users.len(), 3);
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
    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc
        .execute(CreateUserCommand {
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Create multiple groups
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());

    let groups = vec!["Developers", "Designers", "Managers"];

    for group_name in groups {
        let group_view = create_group_uc
            .execute(CreateGroupCommand {
                group_name: group_name.to_string(),
                tags: vec![],
            })
            .await
            .unwrap();

        let result = add_uc
            .execute(AddUserToGroupCommand {
                user_hrn: user_view.hrn.clone(),
                group_hrn: group_view.hrn,
            })
            .await;

        assert!(
            result.is_ok(),
            "Failed to add user to group {}: {:?}",
            group_name,
            result.err()
        );
    }

    // Verify user is in all groups
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 3);
}

#[tokio::test]
async fn test_add_user_to_group_idempotent() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user and group
    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc
        .execute(CreateUserCommand {
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Developers".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Add user to group twice
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());

    let result1 = add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn.clone(),
        })
        .await;

    let result2 = add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn.clone(),
        })
        .await;

    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // Should still only be in the group once
    let user = user_repo
        .find_by_hrn(&kernel::Hrn::from_string(&user_view.hrn).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.groups().len(), 1);
}

#[tokio::test]
async fn test_complex_user_group_relationships() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());

    // Create groups
    let dev_group = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Developers".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    let ops_group = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Operations".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Create users
    let alice = create_user_uc
        .execute(CreateUserCommand {
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    let bob = create_user_uc
        .execute(CreateUserCommand {
            name: "Bob".to_string(),
            email: "bob@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Alice is in both groups
    add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: alice.hrn.clone(),
            group_hrn: dev_group.hrn.clone(),
        })
        .await
        .unwrap();

    add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: alice.hrn,
            group_hrn: ops_group.hrn.clone(),
        })
        .await
        .unwrap();

    // Bob is only in developers
    add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: bob.hrn,
            group_hrn: dev_group.hrn,
        })
        .await
        .unwrap();

    // Verify relationships
    let all_users = user_repo.find_all().await.unwrap();
    let alice_user = all_users.iter().find(|u| u.name == "Alice").unwrap();
    let bob_user = all_users.iter().find(|u| u.name == "Bob").unwrap();

    assert_eq!(alice_user.groups().len(), 2);
    assert_eq!(bob_user.groups().len(), 1);
}

#[tokio::test]
async fn test_add_user_to_nonexistent_group_fails() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a user
    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc
        .execute(CreateUserCommand {
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Try to add user to a non-existent group
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());
    let result = add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn,
            group_hrn: "hrn:hodei:iam::default:Group/nonexistent".to_string(),
        })
        .await;

    // Should fail
    assert!(result.is_err(), "Should fail when group doesn't exist");
}

#[tokio::test]
async fn test_add_nonexistent_user_to_group_fails() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a group
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc
        .execute(CreateGroupCommand {
            group_name: "Developers".to_string(),
            tags: vec![],
        })
        .await
        .unwrap();

    // Try to add a non-existent user to the group
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());
    let result = add_uc
        .execute(AddUserToGroupCommand {
            user_hrn: "hrn:hodei:iam::default:User/nonexistent".to_string(),
            group_hrn: group_view.hrn,
        })
        .await;

    // Should fail
    assert!(result.is_err(), "Should fail when user doesn't exist");
}

#[tokio::test]
async fn test_add_many_users_to_many_groups() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    let create_user_uc = create_user::di::make_use_case(user_repo.clone());
    let create_group_uc = create_group::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_test_use_case(user_repo.clone(), group_repo.clone());

    // Create 5 users
    let mut user_hrns = Vec::new();
    for i in 1..=5 {
        let user = create_user_uc
            .execute(CreateUserCommand {
                name: format!("User{}", i),
                email: format!("user{}@test.com", i),
                tags: vec![],
            })
            .await
            .unwrap();
        user_hrns.push(user.hrn);
    }

    // Create 3 groups
    let mut group_hrns = Vec::new();
    for i in 1..=3 {
        let group = create_group_uc
            .execute(CreateGroupCommand {
                group_name: format!("Group{}", i),
                tags: vec![],
            })
            .await
            .unwrap();
        group_hrns.push(group.hrn);
    }

    // Add each user to each group
    for user_hrn in &user_hrns {
        for group_hrn in &group_hrns {
            let result = add_uc
                .execute(AddUserToGroupCommand {
                    user_hrn: user_hrn.clone(),
                    group_hrn: group_hrn.clone(),
                })
                .await;
            assert!(result.is_ok());
        }
    }

    // Verify all users are in all groups
    let all_users = user_repo.find_all().await.unwrap();
    assert_eq!(all_users.len(), 5);
    for user in all_users {
        assert_eq!(user.groups().len(), 3);
    }
}
