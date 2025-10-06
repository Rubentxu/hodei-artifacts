use crate::internal::domain::OrganizationalUnit;
use kernel::Hrn;

#[test]
fn test_ou_add_child_account() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    let account_hrn = Hrn::new("account", "test-account");
    ou.add_child_account(account_hrn.clone());

    assert!(ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_account() {
    let account_hrn = Hrn::new("account", "test-account");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    ou.add_child_account(account_hrn.clone());
    assert!(ou.child_accounts.contains(&account_hrn.to_string()));

    ou.remove_child_account(account_hrn.clone());
    assert!(!ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_add_child_ou() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    let child_ou_hrn = Hrn::new("ou", "child-ou");
    ou.add_child_ou(child_ou_hrn.clone());

    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_ou() {
    let child_ou_hrn = Hrn::new("ou", "child-ou");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    ou.add_child_ou(child_ou_hrn.clone());
    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));

    ou.remove_child_ou(child_ou_hrn.clone());
    assert!(!ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_attach_scp() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    let scp_hrn = Hrn::new("scp", "test-scp");
    ou.attach_scp(scp_hrn.clone());

    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));
}

#[test]
fn test_ou_detach_scp() {
    let scp_hrn = Hrn::new("scp", "test-scp");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );

    ou.attach_scp(scp_hrn.clone());
    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));

    ou.detach_scp(scp_hrn.clone());
    assert!(!ou.attached_scps.contains(&scp_hrn.to_string()));
}
