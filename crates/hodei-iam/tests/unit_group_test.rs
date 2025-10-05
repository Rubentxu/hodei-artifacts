/// Unit tests for Group domain entity
use hodei_iam::shared::domain::Group;
use kernel::Hrn;

#[test]
fn test_group_new_creates_empty_collections() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.name, "Developers");
    assert_eq!(group.tags.len(), 0);
    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_attach_policy_idempotent() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "policy1".into(),
    );

    // Attach policy twice
    group.attach_policy(policy_hrn.clone());
    group.attach_policy(policy_hrn.clone());

    // Should only have one policy
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy_hrn);
}

#[test]
fn test_group_detach_policy() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );
    let policy2 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p2".into(),
    );

    group.attach_policy(policy1.clone());
    group.attach_policy(policy2.clone());
    assert_eq!(group.attached_policies().len(), 2);

    group.detach_policy(&policy1);
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy2);
}

#[test]
fn test_group_detach_nonexistent_policy_does_nothing() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );

    // Detach policy that doesn't exist
    group.detach_policy(&policy_hrn);

    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_name_getter() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.group_name(), "Developers");
}

#[test]
fn test_group_multiple_policies() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );
    let policy2 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p2".into(),
    );
    let policy3 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p3".into(),
    );

    group.attach_policy(policy1);
    group.attach_policy(policy2);
    group.attach_policy(policy3);

    assert_eq!(group.attached_policies().len(), 3);
}
