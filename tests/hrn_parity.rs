//! Parity fixtures between the persisted kernel HRN and the published HRN
//! contract 0.2.1 used by the authz crate.
//!
//! The kernel type stays for persistence/HTTP compatibility. These fixtures
//! prove that the contract versions parse the same persisted shapes; they do
//! not claim byte-for-byte equality for layouts the old parser never accepted.

use hodei_hrn::Hrn as PublishedHrn;

const PERSISTED_FIXTURES: &[&str] = &[
    "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
    "hrn:hodei:artifacts::acme:repository/releases",
    "hrn:hodei:iam::tenant-a:User/alice",
    "hrn:hodei:repository:eu-west-1:acme:repository/releases",
    "hrn:hodei:artifacts:eu-west-1:acme:repository/releases/v2",
];

#[test]
fn persisted_fixtures_match_the_published_hrn_contract() {
    for fixture in PERSISTED_FIXTURES {
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

#[test]
fn digit_leading_service_is_accepted_by_the_canonical_contract() {
    let hrn = hodei_hrn::Hrn::new("hodei", "1service", "eu", "acme", "repository", "releases")
        .expect("digit-leading service is valid in the contract");
    assert_eq!(hrn.service(), "1service");
}
