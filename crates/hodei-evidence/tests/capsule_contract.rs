//! The contract of the trust capsule and the trust profile.
//!
//! The capsule is the passport of a release: it says, dimension by
//! dimension, which evidence holds the verdict up and which is missing. The
//! M2 UAT is the gate, and these tests are that gate expressed in code.
//!
//! # Guarantees
//!
//! - C1 Every dimension says the identity of its evidence, or says that
//!   there is none. A dimension with no evidence is `MISSING`, and the
//!   absence is a value and not a hole.
//! - C2 The capsule has a content hash and a version, and the same evidence
//!   produces the same capsule. A piece of evidence that arrives later
//!   produces a different capsule and a different hash.
//! - C3 The absent, the stale, the invalid and the contradictory are four
//!   different states, and none of them counts as a good one.
//! - C4 A dimension that could not be established never counts as
//!   established. `MISSING` is not `VERIFIED`.
//! - C5 The evidence that contradicts another claim stays visible: the
//!   capsule does not delete one of them.
//! - C6 The hash covers the inputs and not the moment of the assembly, so
//!   two readers of the same evidence get the same capsule.

use chrono::{DateTime, Duration, TimeZone, Utc};
use hodei_evidence::application::capsule::{CapsuleAssembler, FreshnessThreshold};
use hodei_evidence::domain::capsule::{
    DimensionName, DimensionState, EvidenceLink, TrustCapsule, TrustProfile,
};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use kernel::Hrn;

fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

/// A piece of evidence of a kind, with a verification and a normalizer.
fn evidence(
    id: &str,
    kind: EvidenceType,
    verification: EvidenceVerification,
    normalizer: &str,
    version: &str,
    observed_at: DateTime<Utc>,
) -> EvidenceRecord {
    EvidenceRecord::builder(EvidenceId::new(id), subject())
        .evidence_type(kind)
        .source(SourceRef::by_external_id(format!("external-{id}")))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(
            id.as_bytes(),
        )))
        .observed_at(observed_at)
        .recorded_at(observed_at)
        .verification(verification)
        .normalization(Normalization::new(normalizer, version))
        .producer(producer())
        .build()
}

/// The evidence of a release with everything in place.
fn complete_evidence() -> Vec<EvidenceRecord> {
    let observed = moment(0);
    vec![
        evidence(
            "ev-integrity",
            EvidenceType::ArtifactIntegrity,
            EvidenceVerification::verified("digest"),
            "cyclonedx",
            "1.0.0",
            observed,
        ),
        evidence(
            "ev-sbom",
            EvidenceType::Sbom,
            EvidenceVerification::verified("schema"),
            "cyclonedx",
            "1.0.0",
            observed,
        ),
        evidence(
            "ev-provenance",
            EvidenceType::Provenance,
            EvidenceVerification::verified("dsse"),
            "slsa",
            "1.0.0",
            observed,
        ),
        evidence(
            "ev-signature",
            EvidenceType::SignatureVerification,
            EvidenceVerification::verified("ecdsa-p256"),
            "sigstore",
            "1.0.0",
            observed,
        ),
    ]
}

/// The assembler with a threshold of thirty days.
fn assembler() -> CapsuleAssembler {
    CapsuleAssembler::with_freshness(FreshnessThreshold {
        maximum_age: Duration::days(30),
    })
}

/// C1 Every dimension says its evidence, or says there is none.
#[test]
fn c1_every_dimension_says_which_evidence_holds_it_up() {
    let capsule = assembler().assemble(&subject(), &complete_evidence(), moment(0));

    let integrity = capsule
        .profile
        .dimension(&DimensionName::Integrity)
        .expect("the integrity dimension is there");
    assert_eq!(integrity.state, DimensionState::Verified);
    assert_eq!(integrity.evidence.len(), 1);
    assert_eq!(integrity.evidence[0].evidence_id.as_str(), "ev-integrity");
    assert_eq!(integrity.evidence[0].normalizer, "cyclonedx");
    assert_eq!(integrity.evidence[0].normalizer_version, "1.0.0");
    assert_eq!(
        integrity.evidence[0].verification.status,
        hodei_evidence::domain::evidence::VerificationStatus::Verified
    );

    // The link carries what a reader needs to go and look.
    let link: &EvidenceLink = &integrity.evidence[0];
    assert!(!link.source.external_id.as_deref().unwrap_or("").is_empty());
}

/// C1 A dimension with no evidence says so, and the absence is a value.
#[test]
fn c1_a_dimension_without_evidence_is_missing_and_says_so() {
    let capsule = assembler().assemble(&subject(), &complete_evidence(), moment(0));

    // The assurance of the source is not in the evidence, and the
    // dimension says so instead of being absent. The dimensions that M2
    // covers are always in the profile, with or without evidence: a
    // profile that only shows the dimensions that happen to have evidence
    // cannot answer what is missing.
    let source = capsule
        .profile
        .dimension(&DimensionName::SourceAssurance)
        .expect("the dimension is there");
    assert_eq!(source.state, DimensionState::Missing);
    assert!(
        source.evidence.is_empty(),
        "a missing dimension holds no evidence"
    );
    assert!(
        source.reason.is_some(),
        "a missing dimension says why it is missing"
    );
}

/// C2 The capsule has a hash and a version, and the same evidence gives the
/// same capsule.
#[test]
fn c2_the_same_evidence_gives_the_same_capsule() {
    let records = complete_evidence();
    let first = assembler().assemble(&subject(), &records, moment(0));
    let second = assembler().assemble(&subject(), &records, moment(0));

    assert_eq!(first.content_hash, second.content_hash);
    assert_eq!(first.profile, second.profile);
    assert!(!first.version.is_empty());
}

/// C2 A piece of evidence that arrives later gives a different capsule.
///
/// The hash identifies the inputs, so a capsule under a hash still says what
/// it said when a decision pointed at it.
#[test]
fn c2_a_new_piece_of_evidence_gives_a_new_capsule() {
    let records = complete_evidence();
    let before = assembler().assemble(&subject(), &records, moment(0));

    let mut later = records.clone();
    later.push(evidence(
        "ev-test",
        EvidenceType::TestResult,
        EvidenceVerification::verified("runner"),
        "pipelinek",
        "1.0.0",
        moment(0),
    ));
    let after = assembler().assemble(&subject(), &later, moment(0));

    assert_ne!(
        before.content_hash, after.content_hash,
        "the capsule under the old hash still means the old thing"
    );
    assert_eq!(
        after
            .profile
            .dimension(&DimensionName::TestEvidence)
            .expect("the dimension is there")
            .state,
        DimensionState::Verified
    );
}

/// C6 The hash covers the inputs and not the moment of the assembly.
#[test]
fn c6_the_hash_covers_the_inputs_and_not_the_moment() {
    let records = complete_evidence();
    let early = assembler().assemble(&subject(), &records, moment(0));
    let late = assembler().assemble(&subject(), &records, moment(3600));

    assert_eq!(
        early.content_hash, late.content_hash,
        "two readers of the same evidence get the same capsule"
    );
    assert_ne!(
        early.assembled_at, late.assembled_at,
        "but they know when each one was assembled"
    );
}

/// C3 The absent, the stale, the invalid and the contradictory are four
/// different states.
#[test]
fn c3_the_four_states_of_no_evidence_are_different() {
    let observed = moment(0);
    let now = moment(60 * 60 * 24 * 60); // sixty days later

    // Absent.
    let absent = assembler().assemble(&subject(), &[], now);
    assert_eq!(
        absent
            .profile
            .dimension(&DimensionName::Integrity)
            .expect("the dimension is there")
            .state,
        DimensionState::Missing
    );

    // Stale: the evidence is there but it is old.
    let stale = assembler().assemble(
        &subject(),
        &[evidence(
            "ev-integrity",
            EvidenceType::ArtifactIntegrity,
            EvidenceVerification::verified("digest"),
            "cyclonedx",
            "1.0.0",
            observed,
        )],
        now,
    );
    assert_eq!(
        stale
            .profile
            .dimension(&DimensionName::Integrity)
            .expect("the dimension is there")
            .state,
        DimensionState::Stale
    );
    assert_eq!(
        stale
            .profile
            .dimension(&DimensionName::Integrity)
            .expect("the dimension is there")
            .evidence
            .len(),
        1,
        "the historical reading stays visible"
    );

    // Invalid: the evidence is there and it was checked and it does not
    // hold.
    let invalid = assembler().assemble(
        &subject(),
        &[evidence(
            "ev-integrity",
            EvidenceType::ArtifactIntegrity,
            EvidenceVerification::invalid("digest"),
            "cyclonedx",
            "1.0.0",
            observed,
        )],
        now,
    );
    assert_eq!(
        invalid
            .profile
            .dimension(&DimensionName::Integrity)
            .expect("the dimension is there")
            .state,
        DimensionState::Invalid
    );

    // Contradictory: the evidence says more than one thing.
    let contradictory = assembler().assemble(
        &subject(),
        &[
            evidence(
                "ev-a",
                EvidenceType::ArtifactIntegrity,
                EvidenceVerification::verified("digest"),
                "cyclonedx",
                "1.0.0",
                observed,
            ),
            evidence(
                "ev-b",
                EvidenceType::ArtifactIntegrity,
                EvidenceVerification::invalid("digest"),
                "other",
                "1.0.0",
                observed,
            ),
        ],
        now,
    );
    assert_eq!(
        contradictory
            .profile
            .dimension(&DimensionName::Integrity)
            .expect("the dimension is there")
            .state,
        DimensionState::Contradictory
    );
}

/// C3 The four states are not the same state.
#[test]
fn c3_the_states_are_distinguishable_from_each_other() {
    let states = [
        DimensionState::Missing,
        DimensionState::Stale,
        DimensionState::Invalid,
        DimensionState::Contradictory,
        DimensionState::Unknown,
    ];
    for (index, left) in states.iter().enumerate() {
        for right in states.iter().skip(index + 1) {
            assert_ne!(left, right, "{left} and {right} are different states");
        }
    }
}

/// C4 A dimension that could not be established never counts as
/// established.
#[test]
fn c4_a_dimension_that_cannot_be_established_does_not_count_as_established() {
    let observed = moment(0);
    let now = moment(0);

    // The evidence is there but nobody checked it.
    let unverified = assembler().assemble(
        &subject(),
        &[evidence(
            "ev-integrity",
            EvidenceType::ArtifactIntegrity,
            EvidenceVerification::unverified(),
            "cyclonedx",
            "1.0.0",
            observed,
        )],
        now,
    );
    let dimension = unverified
        .profile
        .dimension(&DimensionName::Integrity)
        .expect("the dimension is there");
    assert_eq!(dimension.state, DimensionState::Unverified);
    assert!(!dimension.is_satisfied());

    // And the profile says which dimensions do not hold up.
    assert!(
        !unverified.profile.unsatisfied().is_empty(),
        "the profile says what is not established"
    );
}

/// C4 The profile never affirms a dimension it has no evidence for.
#[test]
fn c4_the_profile_never_affirms_a_dimension_without_evidence() {
    let capsule = assembler().assemble(&subject(), &complete_evidence(), moment(0));

    for dimension in capsule.profile.dimensions.iter() {
        match dimension.state {
            DimensionState::Verified | DimensionState::Complete => {
                assert!(
                    !dimension.evidence.is_empty(),
                    "{} is affirmed and it has no evidence",
                    dimension.name
                );
            }
            DimensionState::Missing | DimensionState::NotApplicable => {
                assert!(
                    dimension.evidence.is_empty(),
                    "{} has no evidence and it holds some",
                    dimension.name
                );
            }
            _ => {}
        }
    }
}

/// C5 The evidence that contradicts another claim stays visible.
#[test]
fn c5_the_contradicting_claims_stay_visible() {
    let observed = moment(0);
    let capsule = assembler().assemble(
        &subject(),
        &[
            evidence(
                "ev-a",
                EvidenceType::ArtifactIntegrity,
                EvidenceVerification::verified("digest"),
                "cyclonedx",
                "1.0.0",
                observed,
            ),
            evidence(
                "ev-b",
                EvidenceType::ArtifactIntegrity,
                EvidenceVerification::invalid("digest"),
                "other",
                "1.0.0",
                observed,
            ),
        ],
        moment(0),
    );

    let dimension = capsule
        .profile
        .dimension(&DimensionName::Integrity)
        .expect("the dimension is there");
    assert_eq!(dimension.state, DimensionState::Contradictory);
    assert_eq!(
        dimension.evidence.len(),
        2,
        "both claims stay visible: the capsule does not delete one of them"
    );
}

/// The profile of a subject with nothing is a profile with dimensions that
/// are all missing, and not an empty profile.
#[test]
fn a_profile_without_evidence_says_so_dimension_by_dimension() {
    let capsule = assembler().assemble(&subject(), &[], moment(0));

    assert!(
        !capsule.profile.dimensions.is_empty(),
        "the profile has the dimensions, even with no evidence"
    );
    for dimension in &capsule.profile.dimensions {
        assert_ne!(dimension.state, DimensionState::Verified);
        assert!(!dimension.is_satisfied());
    }
}

/// The capsule is a value that can be sent and received whole.
#[test]
fn the_capsule_travels_whole() {
    let capsule = assembler().assemble(&subject(), &complete_evidence(), moment(0));

    let json = serde_json::to_string(&capsule).expect("the capsule is written");
    let read: TrustCapsule = serde_json::from_str(&json).expect("the capsule is read");
    assert_eq!(read, capsule);

    let profile_json = serde_json::to_string(&capsule.profile).expect("the profile is written");
    let read_profile: TrustProfile =
        serde_json::from_str(&profile_json).expect("the profile is read");
    assert_eq!(read_profile, capsule.profile);
}
