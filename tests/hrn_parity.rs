//! Parity fixtures between the persisted kernel HRN, the HRN version resolved
//! by the published authz crate (0.1.0), and the published HRN contract 0.2.0.
//!
//! The kernel type stays for persistence/HTTP compatibility. These fixtures
//! prove that the contract versions parse the same persisted shapes; they do
//! not claim byte-for-byte equality for layouts the old parser never accepted.

use hodei_hrn::Hrn as PublishedHrn;
use hodei_hrn_01::Hrn as AuthzHrn;

const PERSISTED_FIXTURES: &[&str] = &[
    "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
    "hrn:hodei:artifacts::acme:repository/releases",
    "hrn:hodei:iam::tenant-a:User/alice",
    "hrn:hodei:repository:eu-west-1:acme:repository/releases",
];

#[test]
fn persisted_fixtures_match_the_hrn_version_used_by_authz() {
    for fixture in PERSISTED_FIXTURES {
        let persisted = kernel::Hrn::from_string(fixture).expect("kernel HRN");
        let authz: AuthzHrn = fixture.parse().expect("authz HRN 0.1.0");

        assert_eq!(persisted.partition(), authz.partition, "{fixture}");
        assert_eq!(persisted.service(), authz.service, "{fixture}");
        assert_eq!(persisted.region(), authz.region, "{fixture}");
        assert_eq!(persisted.account_id(), authz.tenant_id, "{fixture}");
        assert_eq!(persisted.resource_type(), authz.resource_type, "{fixture}");
        assert_eq!(persisted.resource_id(), authz.resource_id, "{fixture}");
        assert_eq!(persisted.to_string(), authz.to_string(), "{fixture}");
    }
}

#[test]
fn persisted_fixtures_match_the_published_hrn_contract() {
    let fixtures = [
        "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
        "hrn:hodei:artifacts::acme:repository/releases",
        "hrn:hodei:iam::tenant-a:User/alice",
        "hrn:hodei:repository:eu-west-1:acme:repository/releases",
        "hrn:hodei:artifacts:eu-west-1:acme:repository/releases/v2",
    ];

    for fixture in fixtures {
        let persisted = kernel::Hrn::from_string(fixture).expect("kernel HRN");
        let published = PublishedHrn::from_string(fixture).expect("published HRN");

        assert_eq!(persisted.partition(), published.partition(), "{fixture}");
        assert_eq!(persisted.service(), published.service(), "{fixture}");
        assert_eq!(persisted.region(), published.region(), "{fixture}");
        assert_eq!(persisted.account_id(), published.account(), "{fixture}");
        assert_eq!(
            persisted.resource_type(),
            published.resource_type(),
            "{fixture}"
        );
        assert_eq!(
            persisted.resource_id(),
            published.resource_path(),
            "{fixture}"
        );
        assert_eq!(persisted.to_string(), published.to_string(), "{fixture}");
        assert_eq!(persisted.is_legacy_v1(), published.region().is_empty());
    }
}
