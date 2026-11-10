//! Source-aware migration fixtures for persisted HRNs.

use kernel::domain::hrn_migration::{HrnMigrationError, HrnSource, MigratedHrn};
use serde_json::Value;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/hrn_migration.json")).expect("fixtures")
}

#[test]
fn old_fixtures_preserve_account_scope_and_canonical_identity() {
    for fixture in fixtures() {
        let name = fixture["name"].as_str().unwrap();
        let stored = fixture["stored"].as_str().unwrap();
        let expected = fixture["canonical"].as_str().unwrap();
        let source = fixture["source"].as_str().unwrap();

        let migrated = match source {
            "ArtifactsV1" | "ArtifactsV2" => MigratedHrn::from_artifacts(stored).expect(name),
            "AuthzV01" => MigratedHrn::from_authz_v01(stored, "tenant-a").expect(name),
            "LegacyRepository" => MigratedHrn::from_legacy_repository(stored, "acme").expect(name),
            other => panic!("unknown fixture source {other}"),
        };

        assert_eq!(migrated.stored(), stored, "{name}: stored bytes");
        assert_eq!(
            migrated.canonical().to_string(),
            expected,
            "{name}: canonical"
        );
        assert_eq!(
            migrated.source(),
            match source {
                "ArtifactsV1" => HrnSource::ArtifactsV1,
                "ArtifactsV2" => HrnSource::ArtifactsV2,
                "AuthzV01" => HrnSource::AuthzV01,
                "LegacyRepository" => HrnSource::LegacyRepository,
                _ => unreachable!(),
            },
            "{name}: source"
        );
    }
}

#[test]
fn different_accounts_never_share_a_canonical_identity() {
    let acme =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:acme:repository/releases")
            .unwrap();
    let other =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:other:repository/releases")
            .unwrap();

    assert_ne!(
        acme.canonical().scoped_entity_id(),
        other.canonical().scoped_entity_id()
    );
    assert_eq!(acme.canonical().account_id(), "acme");
    assert_eq!(other.canonical().account_id(), "other");
}

#[test]
fn authz_v01_requires_the_expected_account() {
    let stored = "hrn:hodei:iam::tenant-a:User/alice";
    assert!(MigratedHrn::from_authz_v01(stored, "tenant-a").is_ok());
    assert!(matches!(
        MigratedHrn::from_authz_v01(stored, "tenant-b"),
        Err(HrnMigrationError::ScopeMismatch { .. })
    ));
}

#[test]
fn legacy_repository_conversion_is_explicit_and_reversible() {
    let stored = "hrn:hodei:repository:eu-west-1:acme:repository/releases";
    let migrated = MigratedHrn::from_legacy_repository(stored, "acme").unwrap();

    assert_eq!(migrated.canonical().service(), "artifacts");
    assert_eq!(
        migrated.canonical().to_string(),
        "hrn:hodei:artifacts:eu-west-1:acme:repository/releases"
    );
    assert_eq!(migrated.legacy_repository_string().as_deref(), Some(stored));
    assert!(migrated.rewrites_stored_bytes());
}

#[test]
fn from_artifacts_is_strict_about_the_service() {
    assert!(matches!(
        MigratedHrn::from_artifacts("hrn:aws:iam::123:User/alice"),
        Err(HrnMigrationError::NotArtifacts)
    ));
}

#[test]
fn canonical_artifacts_cannot_be_silently_downgraded_to_legacy_repository() {
    let canonical =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:acme:repository/releases")
            .unwrap();
    assert!(canonical.legacy_repository_string().is_none());
}

#[test]
fn a_non_repository_service_is_not_silently_rewritten() {
    assert!(matches!(
        MigratedHrn::from_legacy_repository(
            "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
            "acme"
        ),
        Err(HrnMigrationError::NotLegacyRepository)
    ));
}

#[test]
fn nested_paths_are_preserved_without_reinterpretation() {
    let stored = "hrn:hodei:artifacts:eu-west-1:acme:repository/releases/v2";
    let migrated = MigratedHrn::from_artifacts(stored).unwrap();

    assert_eq!(migrated.canonical().to_string(), stored);
    assert_eq!(migrated.canonical().resource_id(), "releases/v2");
    assert_eq!(migrated.canonical().account_id(), "acme");
}

#[test]
fn the_adapter_exposes_a_typed_hrn_without_parser_details() {
    let migrated =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:acme:repository/releases")
            .unwrap();

    // Consumers receive a typed Hrn; they do not need to re-parse or know
    // about the v1/v2 string layout. Production wiring is HSP-13.
    let canonical: &kernel::Hrn = migrated.canonical();
    assert_eq!(canonical.resource_type(), "repository");
    assert_eq!(canonical.account_id(), "acme");
}

#[test]
fn account_isolation_holds_across_every_source() {
    let artifacts_a =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:acme:repository/releases")
            .unwrap();
    let artifacts_b =
        MigratedHrn::from_artifacts("hrn:hodei:artifacts:eu-west-1:other:repository/releases")
            .unwrap();
    let authz_a = MigratedHrn::from_authz_v01("hrn:hodei:iam::acme:User/alice", "acme").unwrap();
    let authz_b = MigratedHrn::from_authz_v01("hrn:hodei:iam::other:User/alice", "other").unwrap();
    let legacy_a = MigratedHrn::from_legacy_repository(
        "hrn:hodei:repository:eu-west-1:acme:repository/releases",
        "acme",
    )
    .unwrap();
    let legacy_b = MigratedHrn::from_legacy_repository(
        "hrn:hodei:repository:eu-west-1:other:repository/releases",
        "other",
    )
    .unwrap();

    for (left, right) in [
        (&artifacts_a, &artifacts_b),
        (&authz_a, &authz_b),
        (&legacy_a, &legacy_b),
    ] {
        assert_ne!(
            left.canonical().entity_uid_string(),
            right.canonical().entity_uid_string()
        );
    }
}

#[test]
fn invalid_and_wrong_source_inputs_are_rejected() {
    assert!(matches!(
        MigratedHrn::from_artifacts("not-an-hrn"),
        Err(HrnMigrationError::InvalidHrn)
    ));
    assert!(matches!(
        MigratedHrn::from_artifacts("hrn:hodei:iam::acme:User/alice"),
        Err(HrnMigrationError::NotArtifacts)
    ));
    assert!(matches!(
        MigratedHrn::from_authz_v01(
            "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
            "acme"
        ),
        Err(HrnMigrationError::WrongSource)
    ));
    assert!(matches!(
        MigratedHrn::from_legacy_repository(
            "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
            "acme"
        ),
        Err(HrnMigrationError::NotLegacyRepository)
    ));
    assert!(matches!(
        MigratedHrn::from_legacy_repository(
            "hrn:hodei:repository:eu-west-1:other:repository/releases",
            "acme"
        ),
        Err(HrnMigrationError::ScopeMismatch { .. })
    ));
}

#[test]
fn service_case_is_normalized_but_stored_bytes_are_kept() {
    let stored = "hrn:hodei:Artifacts:eu-west-1:acme:repository/releases";
    let migrated = MigratedHrn::from_artifacts(stored).unwrap();

    assert_eq!(migrated.canonical().service(), "artifacts");
    assert_eq!(migrated.stored(), stored);
    assert!(migrated.rewrites_stored_bytes());
}
