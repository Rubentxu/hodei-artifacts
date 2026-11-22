//! The contract of the SLSA ladder: the level is an evaluation, never a
//! file check.
//!
//! AC2 of HAE-21 says the subject and the build identity are verified,
//! and that a level is asserted **only when the relevant requirements
//! were evaluated**. These tests read a real provenance through the real
//! chain and then ask the assessor what it may assert, under different
//! expectations. The point is the shape of a "no": the JSON existing
//! buys nothing, and a requirement nobody evaluated is a visible gap,
//! never a silent pass.

use chrono::{DateTime, Utc};
use hodei_evidence::standards::dsse::VerificationKey;
use hodei_evidence::standards::slsa::{
    RequirementStatus, SlsaBuildLevel, SlsaExpectations, SlsaRequirement, assess_provenance,
};
use hodei_evidence::standards::{ExpectedIdentity, RootRevision, TrustRoots, cosign_verify};

const GOOD: &str = include_str!("fixtures/trust-chain/cosign-good.dsse.json");
const WRONG_DIGEST: &str = include_str!("fixtures/trust-chain/cosign-wrong-digest.dsse.json");
const TRUNCATED: &str = include_str!("fixtures/trust-chain/cosign-truncated.dsse.json");
const ALPHA_POINT: &str = include_str!("fixtures/trust-chain/trust-root-alpha-public-point.hex");

const KEY_ID_ALPHA: &str = "hsp154-root-alpha-v1";
const GOOD_DIGEST: &str = "ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06";
const BUILDER: &str = "https://builders.example/hsp154-builder@v1";

fn point(hex: &str) -> Vec<u8> {
    hex.trim()
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("the hexadecimal is text");
            u8::from_str_radix(text, 16).expect("the hexadecimal is a byte")
        })
        .collect()
}

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("a timestamp")
        .with_timezone(&Utc)
}

fn alpha_roots() -> TrustRoots {
    let mut roots = TrustRoots::new();
    roots.activate(
        RootRevision {
            lineage: "alpha".to_string(),
            revision: 1,
            key: VerificationKey {
                key_id: KEY_ID_ALPHA.to_string(),
                point: point(ALPHA_POINT),
            },
            issuer: Some("registry.example".to_string()),
            activated_at: at("2026-01-01T00:00:00Z"),
            retired_at: None,
            revoked_at: None,
        },
        "alice",
        "the root the deployment ships",
    );
    roots
}

fn expected(digest: &str) -> ExpectedIdentity {
    ExpectedIdentity {
        subject_algorithm: "sha256".to_string(),
        subject_digest: digest.to_string(),
        issuer: Some("registry.example".to_string()),
        builder_id: Some(BUILDER.to_string()),
    }
}

fn status_of(
    assessment: &hodei_evidence::standards::SlsaAssessment,
    requirement: SlsaRequirement,
) -> &RequirementStatus {
    &assessment
        .findings
        .iter()
        .find(|finding| finding.requirement == requirement)
        .unwrap_or_else(|| panic!("the ladder always carries {requirement:?}"))
        .status
}

/// A signed, bound, identity-pinned provenance earns L2 -- and stops
/// there while the L3 requirements are gaps: a gap is not a pass.
#[test]
fn a_verified_provenance_asserts_l2_with_the_l3_gaps_visible() {
    let chain = cosign_verify(
        GOOD,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");
    let expectations = SlsaExpectations {
        trusted_builder_ids: vec![BUILDER.to_string()],
        pinned_issuer: Some("registry.example".to_string()),
        builder_identity_proven: false,
        hermetic_declared_complete: false,
    };

    let assessment = assess_provenance(&chain, &expected(GOOD_DIGEST), &expectations);

    assert_eq!(assessment.asserted_level(), Some(SlsaBuildLevel::L2));
    assert!(assessment.may_assert(SlsaBuildLevel::L1));
    assert!(assessment.may_assert(SlsaBuildLevel::L2));
    assert!(
        !assessment.may_assert(SlsaBuildLevel::L3),
        "the L3 requirements were not evaluated; the level is not asserted"
    );

    // The subject and the build identity are the verified half of the AC.
    assert_eq!(
        *status_of(&assessment, SlsaRequirement::SubjectVerified),
        RequirementStatus::Met
    );
    assert_eq!(
        *status_of(&assessment, SlsaRequirement::BuilderTrusted),
        RequirementStatus::Met
    );
    assert_eq!(
        *status_of(&assessment, SlsaRequirement::IssuerPinned),
        RequirementStatus::Met
    );

    // And the gaps name what is missing, instead of going silently green.
    let gaps = assessment.gaps();
    assert_eq!(
        gaps.len(),
        2,
        "non-falsifiability and hermeticity: {gaps:?}"
    );
    assert!(
        gaps.iter()
            .all(|gap| matches!(gap.status, RequirementStatus::NotEvaluated { .. })),
        "a gap is NotEvaluated, never Met and never NotMet: {gaps:?}"
    );
}

/// A provenance whose subject is not the artifact earns only L1: the
/// signature exists, the binding does not.
#[test]
fn a_provenance_about_the_wrong_artifact_stops_at_l1() {
    let chain = cosign_verify(
        WRONG_DIGEST,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");
    let expectations = SlsaExpectations {
        trusted_builder_ids: vec![BUILDER.to_string()],
        pinned_issuer: Some("registry.example".to_string()),
        ..SlsaExpectations::default()
    };

    let assessment = assess_provenance(&chain, &expected(GOOD_DIGEST), &expectations);

    assert_eq!(assessment.asserted_level(), Some(SlsaBuildLevel::L1));
    assert!(
        !assessment.may_assert(SlsaBuildLevel::L2),
        "the subject was not verified, so L2 cannot be said"
    );
    let unmet = assessment.unmet_at(SlsaBuildLevel::L2);
    assert_eq!(unmet.len(), 1, "the one unmet L2 requirement: {unmet:?}");
    assert_eq!(unmet[0].requirement, SlsaRequirement::SubjectVerified);
}

/// A builder outside the trusted list keeps the ladder at L1 even when
/// the signature and the subject hold.
#[test]
fn an_untrusted_builder_keeps_the_ladder_below_l2() {
    let chain = cosign_verify(
        GOOD,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");
    let expectations = SlsaExpectations {
        trusted_builder_ids: vec!["https://builders.example/someone-else@v9".to_string()],
        pinned_issuer: Some("registry.example".to_string()),
        ..SlsaExpectations::default()
    };

    let assessment = assess_provenance(&chain, &expected(GOOD_DIGEST), &expectations);

    assert_eq!(assessment.asserted_level(), Some(SlsaBuildLevel::L1));
    assert!(matches!(
        status_of(&assessment, SlsaRequirement::BuilderTrusted),
        RequirementStatus::NotMet { .. }
    ));
}

/// The file existing is not the assessment: a signed bundle whose
/// statement cannot be read asserts nothing, not even L1.
#[test]
fn the_mere_existence_of_a_signed_file_asserts_nothing() {
    let chain = cosign_verify(
        TRUNCATED,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    let assessment = assess_provenance(
        &chain,
        &expected(GOOD_DIGEST),
        &SlsaExpectations {
            trusted_builder_ids: vec![BUILDER.to_string()],
            pinned_issuer: Some("registry.example".to_string()),
            ..SlsaExpectations::default()
        },
    );

    assert_eq!(assessment.asserted_level(), None);
    assert!(
        matches!(
            status_of(&assessment, SlsaRequirement::ProvenanceAvailable),
            RequirementStatus::NotMet { .. }
        ),
        "the predicate type is what makes the provenance available, not the file"
    );
    assert!(
        !assessment.may_assert(SlsaBuildLevel::L1),
        "a provenance that cannot be read may not even assert L1"
    );
}

/// When the deployment establishes the L3 requirements elsewhere, the
/// ladder says L3: the level follows the evaluation, in both directions.
#[test]
fn a_fully_established_ladder_asserts_l3() {
    let chain = cosign_verify(
        GOOD,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");
    let expectations = SlsaExpectations {
        trusted_builder_ids: vec![BUILDER.to_string()],
        pinned_issuer: Some("registry.example".to_string()),
        builder_identity_proven: true,
        hermetic_declared_complete: true,
    };

    let assessment = assess_provenance(&chain, &expected(GOOD_DIGEST), &expectations);

    assert_eq!(assessment.asserted_level(), Some(SlsaBuildLevel::L3));
    assert!(
        assessment.gaps().is_empty(),
        "every requirement was evaluated: {:?}",
        assessment.gaps()
    );
}

/// An issuer the chain never pinned against cannot satisfy L2's issuer
/// requirement: the assessor says `NotEvaluated`, not `Met`, when the
/// question was not put to the chain.
#[test]
fn an_unasked_issuer_pin_is_a_gap_not_a_pass() {
    let chain = cosign_verify(
        GOOD,
        &alpha_roots(),
        &expected(GOOD_DIGEST),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");
    // The policy pins an issuer but the chain was run WITHOUT the pin:
    // the verifier never checked it, so the assessor must not claim it.
    let unpinned = ExpectedIdentity {
        issuer: None,
        ..expected(GOOD_DIGEST)
    };

    let assessment = assess_provenance(
        &chain,
        &unpinned,
        &SlsaExpectations {
            trusted_builder_ids: vec![BUILDER.to_string()],
            pinned_issuer: Some("registry.example".to_string()),
            ..SlsaExpectations::default()
        },
    );

    assert!(
        matches!(
            status_of(&assessment, SlsaRequirement::IssuerPinned),
            RequirementStatus::NotEvaluated { .. }
        ),
        "a pin nobody enforced cannot be reported as satisfied"
    );
    assert_eq!(
        assessment.asserted_level(),
        Some(SlsaBuildLevel::L1),
        "without the issuer verdict L2 is not assertable"
    );
}
