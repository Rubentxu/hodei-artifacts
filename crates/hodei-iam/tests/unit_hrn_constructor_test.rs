/// Unit tests for Hrn constructor with HodeiEntityType

use hodei_iam::{Group, User};
use policies::shared::domain::hrn::Hrn;


#[test]
fn test_hrn_for_entity_type_user() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "user123".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam");  // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "User");
    assert_eq!(hrn.resource_id, "user123");
}

#[test]
fn test_hrn_for_entity_type_group() {
    let hrn = Hrn::for_entity_type::<Group>(
        "hodei".to_string(),
        "default".to_string(),
        "group456".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam");  // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "Group");
    assert_eq!(hrn.resource_id, "group456");
}

#[test]
fn test_hrn_for_entity_type_to_string() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "account1".to_string(),
        "alice".to_string(),
    );

    let hrn_str = hrn.to_string();
    assert!(hrn_str.contains(":iam:"));  // service is lowercase in HRN string
    assert!(hrn_str.contains(":User/"));  // resource_type followed by /
    assert!(hrn_str.contains("alice"));
}

#[test]
fn test_hrn_for_entity_type_euid() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "bob".to_string(),
    );

    let euid = hrn.euid();
    let euid_str = format!("{}", euid);

    assert!(euid_str.contains("Iam::User"));  // Cedar namespace is PascalCase
    assert!(euid_str.contains("bob"));
}
