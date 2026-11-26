//! The contract of the risk graph.
//!
//! Six things are promised and six things are checked:
//!
//! - The vulnerability ingestion is compatible with OSV and the fixtures
//!   are versioned in the repository.
//! - The matching of versions and ranges resolves the exact version, the
//!   range and the unknown version.
//! - The AFFECTED_BY relation is traversed, and a transitive dependency is
//!   told apart from a direct one.
//! - The VEX ingestion changes the state of the finding per what it
//!   declares, and an unverified VEX does not silence a finding.
//! - The security case opens with its state, and the verified remediation
//!   closes it.
//! - An artefact without an inventory has an unknown state, never
//!   "unaffected".

use hodei_evidence::domain::graph::RelationType;
use hodei_risk::{
    AffectedState, BlastRadius, CaseState, DependencyKind, Edge, Match, OsvAffected, SecurityCase,
    VerificationCriteria, VexStatus, affected_paths, apply_vex, blast_radius, match_version,
    read_osv, read_vex, state_without_vex,
};
use kernel::Hrn;

const ADVISORY: &str = include_str!("fixtures/osv-advisory.json");
const ADVISORY_UNKNOWN: &str = include_str!("fixtures/osv-advisory-unknown-version.json");
const VEX_NOT_AFFECTED: &str = include_str!("fixtures/vex-not-affected.json");
const VEX_UNVERIFIED_FIXED: &str = include_str!("fixtures/vex-unverified-fixed.json");

fn hrn(text: &str) -> Hrn {
    Hrn::from_string(text).expect("an HRN")
}

/// The affected package of the advisory.
fn affected_of(document: &str) -> OsvAffected {
    let advisory = read_osv(document).expect("the document is read");
    advisory.affected.into_iter().next().expect("it has one")
}

// ───────────────────────────── AC1 ─────────────────────────────

/// The vulnerability ingestion is compatible with OSV and the fixtures
/// are versioned in the repository.
#[test]
fn the_ingestion_is_compatible_with_osv() {
    let advisory = read_osv(ADVISORY).expect("the fixture is read");
    assert_eq!(advisory.id, "GHSA-hodei-test-0001");
    assert_eq!(advisory.cve(), Some("CVE-2024-99999"));
    assert_eq!(advisory.affected.len(), 1);
    assert_eq!(advisory.affected[0].package.ecosystem, "Maven");
    assert_eq!(advisory.affected[0].package.name, "org.example:lib-b");
    assert_eq!(
        advisory.affected[0].package.purl.as_deref(),
        Some("pkg:maven/org.example/lib-b@2.0.0")
    );
    assert_eq!(advisory.affected[0].fixed_version(), Some("2.0.1"));
    assert!(!advisory.is_withdrawn());

    // And the fixture is in the repository, versioned with the code.
    assert!(ADVISORY.contains("GHSA-hodei-test-0001"));
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The matching of versions and ranges resolves the exact version, the
/// range and the unknown version.
#[test]
fn the_matching_resolves_the_exact_version_the_range_and_the_unknown_one() {
    let affected = affected_of(ADVISORY);

    // The exact version.
    assert_eq!(match_version(&affected, "2.0.0"), Match::Affected);
    assert_eq!(match_version(&affected, "1.0.0"), Match::Affected);

    // The range: the fixed version is no longer affected.
    assert_eq!(match_version(&affected, "2.0.1"), Match::Unaffected);
    assert_eq!(match_version(&affected, "3.0.0"), Match::Unaffected);

    // And the unknown version: it is NOT "unaffected".
    assert_eq!(
        match_version(&affected, "2024.05.01-SNAPSHOT"),
        Match::Unknown,
        "a version that nobody can order is not a version nobody is affected by"
    );

    // The range that ends at last_affected.
    let affected = affected_of(ADVISORY_UNKNOWN);
    assert_eq!(match_version(&affected, "3.0.9"), Match::Affected);
    assert_eq!(match_version(&affected, "3.1.0"), Match::Unaffected);
}

// ───────────────────────────── AC3 ─────────────────────────────

/// The AFFECTED_BY relation is traversed, and a transitive dependency is
/// told apart from a direct one.
#[test]
fn the_affected_by_relation_is_traversed_and_the_depth_is_told() {
    let graph = vec![
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-a"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p1"),
            relation: RelationType::Contains,
        },
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p1"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
            relation: RelationType::DependsOn,
        },
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v"),
            relation: RelationType::AffectedBy,
        },
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-a"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Deployment/production"),
            relation: RelationType::DeployedAs,
        },
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-a"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Deployment/staging"),
            relation: RelationType::DeployedAs,
        },
        Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-b"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p3"),
            relation: RelationType::Contains,
        },
    ];

    let vulnerability = hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v");
    let paths = affected_paths(&graph, &vulnerability);

    assert_eq!(paths.len(), 1, "A is affected and B is not");
    let path = &paths[0];
    assert_eq!(
        path.artifact,
        hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-a")
    );
    assert_eq!(
        path.component,
        hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2")
    );
    assert_eq!(path.depth, 2, "the dependency is transitive");
    assert!(path.is_transitive());
    assert!(!path.is_direct());
    assert_eq!(path.deployments.len(), 2, "and it runs in two environments");

    // And a direct dependency is one step.
    let mut graph = graph.clone();
    graph.push(Edge {
        from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-c"),
        to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
        relation: RelationType::Contains,
    });
    let paths = affected_paths(&graph, &vulnerability);
    let direct = paths
        .iter()
        .find(|path| path.artifact == hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-c"))
        .expect("the direct one is there");
    assert_eq!(direct.depth, 1);
    assert!(direct.is_direct());

    // And the blast radius names what is affected.
    let radius = blast_radius(&graph, &vulnerability);
    assert_eq!(radius.affected_subjects.len(), 2);
    assert_eq!(radius.affected_components.len(), 1);
    assert_eq!(radius.affected_deployments.len(), 2);
}

// ───────────────────────────── AC4 ─────────────────────────────

/// The VEX ingestion changes the state of the finding per what it declares, and
/// an unverified VEX does not silence a finding.
#[test]
fn the_vex_changes_the_state_and_an_unverified_one_does_not_silence_it() {
    // The declaration that says it does not affect.
    let statement = read_vex(VEX_NOT_AFFECTED, "CVE-2024-99999").expect("the fixture is read");
    assert_eq!(statement.status, VexStatus::NotAffected);
    assert!(
        statement
            .products
            .iter()
            .any(|product| product.contains("lib-b"))
    );

    let mut finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Affected,
        DependencyKind::Transitive,
    );
    assert_eq!(finding.state, AffectedState::Affected);

    // With the verified declaration, the finding is no longer affected and
    // the declaration stays alongside.
    let mut verified = statement.clone();
    verified.verified = true;
    apply_vex(&mut finding, &verified);
    assert_eq!(finding.state, AffectedState::Unaffected);
    let reference = finding.vex.clone().expect("the reference is there");
    assert_eq!(reference.id, statement.id);
    assert!(reference.verified);

    // And with a declaration that is NOT verified, the finding is still
    // there: a declaration that nobody checked is a declaration that
    // someone wrote.
    let mut finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Affected,
        DependencyKind::Transitive,
    );
    let unverified = read_vex(VEX_UNVERIFIED_FIXED, "CVE-2024-99999").expect("the fixture is read");
    assert_eq!(unverified.status, VexStatus::Fixed);
    assert!(!unverified.is_verified());

    apply_vex(&mut finding, &unverified);
    assert_eq!(
        finding.state,
        AffectedState::Affected,
        "an unverified claim that something is fixed does not silence a finding"
    );
    assert!(
        finding.vex.is_none(),
        "and it is not recorded as having moved it"
    );
}

// ───────────────────────────── AC5 ─────────────────────────────

/// The security case opens with its state, and the verified
/// remediation closes it.
#[test]
fn the_security_case_opens_with_its_state_and_the_verified_remediation_closes_it() {
    let now = chrono::Utc::now();
    let finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Affected,
        DependencyKind::Transitive,
    );

    let mut case = SecurityCase::open(
        SecurityCase::id_for("GHSA-hodei-test-0001"),
        "GHSA-hodei-test-0001",
        vec![finding.clone()],
        BlastRadius {
            affected_subjects: vec![
                "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-a".to_string(),
            ],
            depths: Vec::new(),
            affected_deployments: vec!["production".to_string()],
            affected_components: vec!["org.example:lib-b".to_string()],
        },
        VerificationCriteria::new(Some("sha256:fixed".to_string()))
            .with_deployments_removed(vec!["production".to_string()]),
        now,
    );

    assert_eq!(case.state, CaseState::Open);
    assert_eq!(case.trigger, "GHSA-hodei-test-0001");
    assert_eq!(case.findings.len(), 1);

    // And the verified remediation closes it.
    let state = case.verify_remediation("sha256:fixed", &["production".to_string()], now);
    assert_eq!(state, CaseState::VerifiedFixed);
    assert!(case.state.is_closed());
    assert!(
        case.timeline
            .iter()
            .any(|event| matches!(event.kind, hodei_risk::CaseEventKind::Verified)),
        "and the verification is in the timeline"
    );
}

// ───────────────────────────── AC6 ─────────────────────────────

/// An artifact without an SBOM has an unknown state, not "unaffected".
#[test]
fn an_artifact_without_an_inventory_has_an_unknown_state() {
    let finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        None,
        false,
        Match::Unaffected,
        DependencyKind::Direct,
    );
    assert_eq!(
        finding.state,
        AffectedState::Unknown,
        "without an inventory nobody knows, and that is not 'unaffected'"
    );
    assert!(finding.state.is_unknown());

    // And with an inventory, the state is the one the matching says.
    let finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Unaffected,
        DependencyKind::Direct,
    );
    assert_eq!(finding.state, AffectedState::Unaffected);

    // And with an inventory and a version that cannot be compared, it is also
    // unknown.
    let finding = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("SNAPSHOT"),
        true,
        Match::Unknown,
        DependencyKind::Direct,
    );
    assert_eq!(finding.state, AffectedState::Unknown);
}

/// And the new evidence updates the case without erasing the history.
#[test]
fn the_new_evidence_updates_the_case_without_erasing_the_history() {
    let now = chrono::Utc::now();
    let affected = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Affected,
        DependencyKind::Transitive,
    );
    let mut case = SecurityCase::open(
        "case-1",
        "GHSA-hodei-test-0001",
        vec![affected],
        BlastRadius {
            affected_subjects: Vec::new(),
            depths: Vec::new(),
            affected_deployments: Vec::new(),
            affected_components: Vec::new(),
        },
        VerificationCriteria::new(None),
        now,
    );

    let mut unaffected = state_without_vex(
        "GHSA-hodei-test-0001",
        "org.example:lib-b",
        Some("2.0.0"),
        true,
        Match::Unaffected,
        DependencyKind::Transitive,
    );
    unaffected.vex = Some(hodei_risk::VexReference {
        id: "vex-1".to_string(),
        status: VexStatus::NotAffected,
        verified: true,
    });

    case.record_evidence(unaffected, "a VEX statement says it is not affected", now);

    assert_eq!(
        case.current_state(),
        CaseState::ClosedNotAffected,
        "the state derives from the new evidence"
    );
    assert_eq!(
        case.timeline.len(),
        3,
        "and the timeline has the opening, the evidence and the change"
    );
    assert_eq!(
        case.trigger, "GHSA-hodei-test-0001",
        "the trigger is still there"
    );
    assert_eq!(case.timeline[0].kind, hodei_risk::CaseEventKind::Opened);
}
