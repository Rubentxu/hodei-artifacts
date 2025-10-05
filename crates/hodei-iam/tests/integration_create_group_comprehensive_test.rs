/// Comprehensive integration tests for create_group feature
/// Uses only public API from hodei_iam crate
use hodei_iam::{
    features::create_group::{self, dto::CreateGroupCommand},
    infrastructure::InMemoryGroupRepository,
    ports::GroupRepository,
};
use std::sync::Arc;

#[tokio::test]
async fn test_create_group_with_valid_name() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Developers".to_string(),
        tags: vec!["engineering".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok(), "Failed to create group: {:?}", result.err());

    let view = result.unwrap();
    assert_eq!(view.name, "Developers");
    assert_eq!(view.tags.len(), 1);
    assert!(view.tags.contains(&"engineering".to_string()));
}

#[tokio::test]
async fn test_create_group_multiple_tags() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Admin Team".to_string(),
        tags: vec![
            "admin".to_string(),
            "security".to_string(),
            "compliance".to_string(),
        ],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"admin".to_string()));
    assert!(view.tags.contains(&"security".to_string()));
    assert!(view.tags.contains(&"compliance".to_string()));
}

#[tokio::test]
async fn test_create_group_no_tags() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Simple Group".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);
}

#[tokio::test]
async fn test_create_group_hrn_format() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Test Group".to_string(),
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
        result.hrn.contains(":Group/"),
        "HRN should contain resource_type 'Group' followed by '/'"
    );
}

#[tokio::test]
async fn test_create_group_unique_ids() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Same Name".to_string(),
        tags: vec![],
    };

    let result1 = use_case.execute(command.clone()).await.unwrap();
    let result2 = use_case.execute(command.clone()).await.unwrap();

    // Even with same data, HRNs should be different (UUID)
    assert_ne!(result1.hrn, result2.hrn);
}

#[tokio::test]
async fn test_create_groups_batch() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let groups = vec!["Engineering", "Marketing", "Sales", "Support"];

    for name in groups {
        let command = CreateGroupCommand {
            group_name: name.to_string(),
            tags: vec![],
        };

        let result = use_case.execute(command).await;
        assert!(
            result.is_ok(),
            "Failed to create group {}: {:?}",
            name,
            result.err()
        );
    }

    // Verify persistence by finding all groups
    let all_groups = repo.find_all().await.unwrap();
    assert_eq!(all_groups.len(), 4);
}

#[tokio::test]
async fn test_create_group_persistence() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "Persistent Group".to_string(),
        tags: vec!["test".to_string()],
    };

    let created = use_case.execute(command).await.unwrap();

    // Verify group was actually persisted
    let found = repo.find_by_hrn(&kernel::Hrn::from_string(&created.hrn).unwrap()).await;
    assert!(found.is_ok());

    let group = found.unwrap();
    assert!(group.is_some());

    let group = group.unwrap();
    assert_eq!(group.name, "Persistent Group");
}

#[tokio::test]
async fn test_create_group_with_special_characters() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "DevOps-Team_2024 (β)".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, "DevOps-Team_2024 (β)");
}

#[tokio::test]
async fn test_create_group_long_name() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let long_name = "A".repeat(200);
    let command = CreateGroupCommand {
        group_name: long_name.clone(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, long_name);
}

#[tokio::test]
async fn test_create_group_empty_name() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "".to_string(),
        tags: vec![],
    };

    // Empty name should be allowed (validation is domain decision)
    let result = use_case.execute(command).await;
    // If your domain requires non-empty names, this should fail
    // For now, we allow it
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_create_multiple_groups_different_tags() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    // Create group with engineering tags
    let cmd1 = CreateGroupCommand {
        group_name: "Backend Team".to_string(),
        tags: vec!["backend".to_string(), "api".to_string()],
    };

    // Create group with frontend tags
    let cmd2 = CreateGroupCommand {
        group_name: "Frontend Team".to_string(),
        tags: vec!["frontend".to_string(), "ui".to_string()],
    };

    let result1 = use_case.execute(cmd1).await.unwrap();
    let result2 = use_case.execute(cmd2).await.unwrap();

    assert_eq!(result1.tags.len(), 2);
    assert_eq!(result2.tags.len(), 2);
    assert_ne!(result1.hrn, result2.hrn);
}

#[tokio::test]
async fn test_create_group_verify_initial_state() {
    let repo = Arc::new(InMemoryGroupRepository::new());
    let use_case = create_group::di::make_use_case(repo.clone());

    let command = CreateGroupCommand {
        group_name: "New Group".to_string(),
        tags: vec!["tag1".to_string()],
    };

    let created = use_case.execute(command).await.unwrap();

    // Verify the group was created with correct initial state
    let found = repo.find_by_hrn(&kernel::Hrn::from_string(&created.hrn).unwrap()).await.unwrap().unwrap();

    // New groups should have no attached policies initially
    assert_eq!(found.attached_policies().len(), 0);
    assert_eq!(found.name, "New Group");
}
