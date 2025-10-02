/// Unit tests for User domain entity

use hodei_iam::User;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_user_new_creates_empty_groups() {
    let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(hrn, "Alice".to_string(), "alice@test.com".to_string());

    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@test.com");
    assert_eq!(user.groups().len(), 0);
    assert_eq!(user.tags.len(), 0);
}

#[test]
fn test_user_add_to_group_idempotent() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "devs".into(),
    );

    // Add group twice
    user.add_to_group(group_hrn.clone());
    user.add_to_group(group_hrn.clone());

    // Should only have one group
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group_hrn);
}

#[test]
fn test_user_remove_from_group() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group1 = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "devs".into());
    let group2 = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "ops".into());

    user.add_to_group(group1.clone());
    user.add_to_group(group2.clone());
    assert_eq!(user.groups().len(), 2);

    user.remove_from_group(&group1);
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group2);
}

#[test]
fn test_user_remove_nonexistent_group_does_nothing() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "devs".into());

    // Remove group that doesn't exist
    user.remove_from_group(&group_hrn);

    assert_eq!(user.groups().len(), 0);
}

#[test]
fn test_user_email_getter() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(user_hrn, "Alice".to_string(), "alice@example.com".to_string());

    assert_eq!(user.email(), "alice@example.com");
}
