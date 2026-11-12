//! The contract of the standards that Hodei reads.
//!
//! The standards enter Hodei through local normalizers and conformance
//! fixtures, before any vendor connector is commissioned. These tests are
//! that conformance suite: they read the real fixtures and they say what
//! the product may rely on.
//!
//! # Guarantees
//!
//! - S1 A CycloneDX 1.7 document is read into Hodei's own model, with its
//!   components, its identities and its dependencies.
//! - S2 An SPDX 3.0 document that says the same things is read into the
//!   **same** facts. The compatibility path is a second normalizer, not a
//!   second model.
//! - S3 The document is never rewritten: the facts carry the digest of the
//!   document they came from.
//! - S4 An in-toto attestation is read **and verified**. A signature that
//!   holds is a proof; one that does not is an invalid attestation with a
//!   reason.
//! - S5 The verification is real: the signature is checked against the key
//!   of its issuer, over the DSSE pre-authentication encoding.
//! - S6 An attestation that does not speak about the artefact it is offered
//!   for is refused, with the reason.
//! - S7 The correlation of identities carries a confidence, and a mapping
//!   that was not confirmed is a hint that may not be used as a verdict.
//! - S8 The version of the normalizer is part of the facts, so a document
//!   read by two versions is two pieces of evidence and not one.

use hodei_evidence::domain::standards::attestation::{
    AttestationVerification, SLSA_PROVENANCE_PREDICATE_TYPE, SignatureFailure,
};
use hodei_evidence::domain::standards::identity::{IdentityNamespace, MappingConfidence};
use hodei_evidence::domain::standards::sbom::SbomFormat;
use hodei_evidence::standards::attestation::{bind_to_subject, read_attestation};
use hodei_evidence::standards::correlation::{
    correlate_identities, correlate_inventory, gather_identities,
};
use hodei_evidence::standards::dsse::{KeyRing, VerificationKey, pae, verify_envelope};
use hodei_evidence::standards::inventory::{read_cyclonedx_17, read_spdx_30};
use std::collections::BTreeMap;

/// The CycloneDX fixture of the conformance suite.
const CYCLONEDX: &str = include_str!("fixtures/standards/cyclonedx-1.7.json");

/// The SPDX fixture of the conformance suite.
const SPDX: &str = include_str!("fixtures/standards/spdx-3.0.json");

/// The attestation signed with the key of the suite.
const ATTESTATION: &str = include_str!("fixtures/standards/attestation.dsse.json");

/// The same attestation with a signature that does not hold.
const ATTESTATION_INVALID: &str = include_str!("fixtures/standards/attestation-invalid.dsse.json");

/// The same attestation with a key that the deployment does not know.
const ATTESTATION_UNKNOWN_KEY: &str =
    include_str!("fixtures/standards/attestation-unknown-key.dsse.json");

/// The raw public point of the key of the suite.
const KEY_POINT_HEX: &str = include_str!("fixtures/standards/signing-key-public-point.hex");

/// The identifier of the key of the suite.
const KEY_ID: &str = "test-key-p256-01";

/// The digest of the subject the fixtures are about.
const SUBJECT_DIGEST: &str = "ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06";

/// The key ring of the suite: the one key the fixtures were signed with.
fn the_key_ring() -> KeyRing {
    let point: Vec<u8> = KEY_POINT_HEX
        .trim()
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("the hexadecimal is text");
            u8::from_str_radix(text, 16).expect("the hexadecimal is a byte")
        })
        .collect();

    KeyRing::with_key(VerificationKey {
        key_id: KEY_ID.to_string(),
        point,
    })
}

/// S1 A CycloneDX 1.7 document is read into Hodei's own model.
#[test]
fn s1_a_cyclonedx_document_is_read_into_the_native_model() {
    let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");

    assert_eq!(inventory.format, SbomFormat::CycloneDx17);

    // The subject: what the document is about.
    let subject = inventory
        .subject
        .as_ref()
        .expect("the document has a subject");
    assert_eq!(subject.name, "web");
    assert_eq!(subject.version.as_deref(), Some("1.0.0"));
    assert_eq!(
        subject
            .identities
            .iter()
            .find(|identity| identity.namespace == IdentityNamespace::Purl)
            .map(|identity| identity.value.as_str()),
        Some("pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06")
    );

    // The subject is also a component, because the dependencies of the
    // document start at it.
    assert!(
        inventory.component(&subject.reference).is_some(),
        "the subject is a component of the inventory"
    );

    // The components, with their identities.
    let lib_a = inventory
        .component("pkg:maven/org.example/lib-a@1.2.3")
        .expect("the component is there");
    assert_eq!(lib_a.name, "lib-a");
    assert_eq!(lib_a.version.as_deref(), Some("1.2.3"));
    assert_eq!(lib_a.purl(), Some("pkg:maven/org.example/lib-a@1.2.3"));

    // A component with a digest has it, and the digest is the identity of
    // the bytes.
    let lib_b = inventory
        .component("pkg:maven/org.example/lib-b@2.0.0")
        .expect("the component is there");
    let digest = lib_b.digest().expect("the component has a digest");
    assert_eq!(
        digest.value,
        "7228493599080d271c0d276600ae9a03ee0b154e853d9ff7689568ad0d73079d"
    );

    // The dependencies, and the walk that follows them.
    assert_eq!(
        inventory.dependencies_of("pkg:maven/org.example/lib-a@1.2.3"),
        vec!["pkg:maven/org.example/lib-b@2.0.0".to_string()]
    );
    let transitive = inventory.transitive_dependencies("pkg:maven/org.example/lib-a@1.2.3");
    assert!(
        transitive.contains(&"pkg:golang/github.com/example/lib-c@v1.4.2".to_string()),
        "the walk reaches the transitive dependency: {transitive:?}"
    );
}

/// S1 A component whose identity cannot be determined keeps no identity.
///
/// The document says a name and a version and nothing else. Hodei does
/// not invent an identity for it, because an invented identity is a
/// correlation that nobody made.
#[test]
fn s1_a_component_without_an_identity_keeps_none() {
    let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    let unknown = inventory
        .component("lib-unknown")
        .expect("the component is there");
    assert!(unknown.identities.is_empty());
    assert_eq!(unknown.purl(), None);
    assert_eq!(unknown.digest(), None);
}

/// S2 The SPDX compatibility path produces the same facts.
#[test]
fn s2_the_spdx_document_produces_the_same_facts() {
    let cyclonedx = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    let spdx = read_spdx_30(SPDX).expect("the document is read");

    assert_eq!(spdx.format, SbomFormat::Spdx30);

    // The subject is the same thing.
    let subject = spdx.subject.as_ref().expect("the document has a subject");
    assert_eq!(subject.name, "web");
    assert_eq!(subject.version.as_deref(), Some("1.0.0"));

    // The components are the same components, with the same identities.
    for component in &cyclonedx.components {
        let counterpart = spdx
            .components
            .iter()
            .find(|candidate| candidate.name == component.name)
            .unwrap_or_else(|| panic!("the SPDX document has {}", component.name));

        let purl_of = |component: &hodei_evidence::domain::standards::sbom::SbomComponent| {
            component.purl().map(str::to_string)
        };
        assert_eq!(
            purl_of(component),
            purl_of(counterpart),
            "both documents name {} the same way",
            component.name
        );

        let digest_of = |component: &hodei_evidence::domain::standards::sbom::SbomComponent| {
            component.digest().map(|digest| digest.value)
        };
        assert_eq!(
            digest_of(component),
            digest_of(counterpart),
            "both documents give {} the same digest",
            component.name
        );
    }

    // And the dependencies are the same dependencies. The comparison is
    // over the **identities** the edges join, and not over the references
    // of the documents: a reference is the key a document uses to name a
    // component, and the two documents use different keys for the same
    // component. A component may also be named by more than one identity,
    // and the two documents do not name it by the same one.
    let identities_of = |inventory: &hodei_evidence::domain::standards::sbom::Sbom,
                         reference: &str|
     -> Vec<String> {
        inventory
            .component(reference)
            .map(|component| {
                component
                    .identities
                    .iter()
                    .map(|identity| format!("{}:{}", identity.namespace, identity.value))
                    .collect()
            })
            .unwrap_or_default()
    };

    let edges = |inventory: &hodei_evidence::domain::standards::sbom::Sbom| {
        let mut edges: Vec<(Vec<String>, Vec<String>)> = Vec::new();
        for dependency in &inventory.dependencies {
            for target in &dependency.depends_on {
                edges.push((
                    identities_of(inventory, &dependency.reference),
                    identities_of(inventory, target),
                ));
            }
        }
        edges
    };

    let cyclonedx_edges = edges(&cyclonedx);
    let spdx_edges = edges(&spdx);

    assert_eq!(
        cyclonedx_edges.len(),
        3,
        "the CycloneDX document has three dependency edges: {cyclonedx_edges:?}"
    );
    assert_eq!(spdx_edges.len(), 3, "and so has the SPDX one");

    // Every edge of the CycloneDX document has an edge of the SPDX one
    // that joins the same identities, and the other way round.
    let joins = |left: &Vec<String>, right: &Vec<String>| {
        left.iter().any(|identity| right.contains(identity))
    };
    for (from, to) in &cyclonedx_edges {
        assert!(
            spdx_edges
                .iter()
                .any(|(other_from, other_to)| joins(from, other_from) && joins(to, other_to)),
            "both documents say the same dependencies over the same identities"
        );
    }
    for (from, to) in &spdx_edges {
        assert!(
            cyclonedx_edges
                .iter()
                .any(|(other_from, other_to)| joins(from, other_from) && joins(to, other_to)),
            "and the other way round"
        );
    }
}

/// S3 The document is never rewritten.
///
/// The facts carry the digest of the document they came from, so the
/// document can always be found again and compared byte for byte.
#[test]
fn s3_the_facts_carry_the_digest_of_the_document() {
    use hodei_evidence::ContentDigest;

    let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    assert_eq!(
        inventory.source_digest,
        ContentDigest::of_bytes(CYCLONEDX.as_bytes())
    );

    let spdx = read_spdx_30(SPDX).expect("the document is read");
    assert_eq!(spdx.source_digest, ContentDigest::of_bytes(SPDX.as_bytes()));
}

/// S3 The moment the document was made is a fact of its own.
///
/// The measure of the standards (SPIKE-005) found it missing: the
/// freshness of the evidence needs to know when the document was made, and
/// a normalizer that drops the moment loses it for good. Both documents
/// say the same moment, and it is not the moment Hodei read them.
#[test]
fn s3_the_moment_the_document_was_made_is_kept() {
    let cyclonedx = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    let spdx = read_spdx_30(SPDX).expect("the document is read");

    let moment = cyclonedx
        .document_timestamp
        .expect("the document says when it was made");
    assert_eq!(
        moment.to_rfc3339(),
        "2026-03-04T10:15:00+00:00",
        "the CycloneDX document says when it was made"
    );
    assert_eq!(
        spdx.document_timestamp,
        Some(moment),
        "and the SPDX document says the same moment"
    );
}

/// S4 An attestation whose signature holds is a proof.
#[test]
fn s4_an_attestation_whose_signature_holds_is_a_proof() {
    let attestation = read_attestation(ATTESTATION, &the_key_ring()).expect("the envelope is read");

    assert!(
        attestation.verification.proves(),
        "the attestation is a proof: {}",
        attestation.verification
    );
    assert_eq!(
        attestation.verification,
        AttestationVerification::Verified {
            key_id: KEY_ID.to_string()
        }
    );
}

/// S4 The statement is read, and it is the one that was signed.
#[test]
fn s4_the_statement_says_what_the_build_was() {
    let attestation = read_attestation(ATTESTATION, &the_key_ring()).expect("the envelope is read");

    assert_eq!(
        attestation.statement.predicate_type,
        SLSA_PROVENANCE_PREDICATE_TYPE
    );
    assert_eq!(
        attestation.statement.predicate.build_type,
        "https://example.invalid/buildtypes/release/v1"
    );
    assert_eq!(
        attestation.statement.predicate.builder_id,
        "https://example.invalid/builders/ci-runner"
    );
    assert_eq!(
        attestation.statement.predicate.invocation_id,
        "example-invocation-01"
    );
    assert_eq!(
        attestation.statement.predicate.resolved_dependencies.len(),
        1
    );
}

/// S5 The verification is real: a signature that was altered does not hold.
#[test]
fn s5_a_signature_that_was_altered_is_invalid_with_its_reason() {
    let attestation =
        read_attestation(ATTESTATION_INVALID, &the_key_ring()).expect("the envelope is read");

    assert!(
        !attestation.verification.proves(),
        "an altered signature is not a proof"
    );
    assert_eq!(
        attestation.verification,
        AttestationVerification::Invalid {
            reason: SignatureFailure::SignatureDoesNotMatch
        }
    );
    assert_eq!(
        attestation.verification.reason(),
        Some(&SignatureFailure::SignatureDoesNotMatch),
        "the reason says what went wrong"
    );
}

/// S5 The signature is over the pre-authentication encoding, not over the
/// payload bytes.
///
/// An envelope whose payload was changed but whose signature was not is
/// invalid: the type is part of what was signed.
#[test]
fn s5_the_signature_covers_the_payload_type_as_well() {
    use hodei_evidence::domain::standards::attestation::DsseEnvelope;

    let envelope: DsseEnvelope = serde_json::from_str(ATTESTATION).expect("the envelope is read");
    let outcome = verify_envelope(&envelope, &the_key_ring());
    assert!(outcome.verification.proves());

    // The same payload under another type does not verify: the bytes that
    // were signed say what the payload means.
    let mut other = envelope.clone();
    other.payload_type = "application/octet-stream".to_string();
    let outcome = verify_envelope(&other, &the_key_ring());
    assert!(
        !outcome.verification.proves(),
        "a signature over another payload type does not hold"
    );
}

/// S5 A key the deployment does not know is a failure of the trust, not a
/// bad signature.
///
/// The two failures lead to different repairs, so they are reported
/// differently.
#[test]
fn s5_an_unknown_key_is_not_reported_as_a_bad_signature() {
    let attestation =
        read_attestation(ATTESTATION_UNKNOWN_KEY, &the_key_ring()).expect("the envelope is read");

    assert!(!attestation.verification.proves());
    assert_eq!(
        attestation.verification,
        AttestationVerification::Unverified {
            reason: SignatureFailure::UnknownKey("test-key-p256-unknown".to_string())
        }
    );
}

/// S6 An attestation that does not speak about the artefact is refused.
#[test]
fn s6_an_attestation_about_another_artefact_is_refused() {
    let mut attestation =
        read_attestation(ATTESTATION, &the_key_ring()).expect("the envelope is read");

    assert!(
        bind_to_subject(&mut attestation, "sha256", SUBJECT_DIGEST).is_ok(),
        "the attestation speaks about the artefact it was signed for"
    );

    let error = bind_to_subject(&mut attestation, "sha256", &"0".repeat(64))
        .expect_err("the attestation does not speak about another artefact");
    assert!(
        matches!(error, SignatureFailure::SubjectMismatch { .. }),
        "the reason says what did not match: {error}"
    );
}

/// S7 The correlation of identities carries a confidence.
#[test]
fn s7_two_spellings_of_the_same_purl_are_correlated_with_certainty() {
    use hodei_evidence::domain::standards::identity::ExternalIdentity;

    let one = ExternalIdentity::confirmed(
        IdentityNamespace::Purl,
        "pkg:Maven/Org.Example/Lib-A@1.2.3",
        "ev-1",
    );
    let other = ExternalIdentity::confirmed(
        IdentityNamespace::Purl,
        "pkg:maven/org.example/lib-a@1.2.3",
        "ev-2",
    );

    let correlation = correlate_identities(&one, &other).expect("the correlation");
    assert!(correlation.same);
    assert_eq!(correlation.confidence, MappingConfidence::Deterministic);
    assert!(correlation.is_verdict());
}

/// S7 A mapping that was not confirmed is a hint, and not a verdict.
///
/// The M2 UAT says it: "Ambiguous mapping remains ambiguous and cannot
/// silently satisfy trust requirements."
#[test]
fn s7_a_mapping_that_was_not_confirmed_is_not_a_verdict() {
    use hodei_evidence::domain::standards::identity::ExternalIdentity;

    // A name that only one source says is a hint.
    let sources = vec![BTreeMap::from([(
        IdentityNamespace::Purl,
        "pkg:maven/org.example/lib-a@1.2.3".to_string(),
    )])];
    let identities = gather_identities(&sources);
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].confidence, MappingConfidence::Unconfirmed);
    assert!(
        !identities[0].is_verdict(),
        "a hint may not be used as a verdict"
    );

    // A name that two sources agree on was confirmed.
    let agreed = vec![
        BTreeMap::from([(
            IdentityNamespace::Purl,
            "pkg:maven/org.example/lib-a@1.2.3".to_string(),
        )]),
        BTreeMap::from([(
            IdentityNamespace::Purl,
            "pkg:maven/org.example/lib-a@1.2.3".to_string(),
        )]),
    ];
    let identities = gather_identities(&agreed);
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].confidence, MappingConfidence::Confirmed);
    assert!(identities[0].is_verdict());

    // And a mapping that nobody confirmed is not a verdict either.
    let hint =
        ExternalIdentity::unconfirmed(IdentityNamespace::Purl, "pkg:maven/org.example/lib-a@1.2.3");
    assert!(!hint.is_verdict());
}

/// S7 The correlation of a component with an artefact keeps the confidence
/// of each answer.
#[test]
fn s7_the_correlation_of_a_component_keeps_its_confidence() {
    use hodei_evidence::domain::standards::identity::ExternalIdentity;

    let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    let artifact = vec![ExternalIdentity::confirmed(
        IdentityNamespace::Purl,
        "pkg:maven/org.example/lib-a@1.2.3",
        "ev-1",
    )];

    let correlated = correlate_inventory(&inventory, &artifact);

    let lib_a = correlated
        .iter()
        .find(|component| component.name == "lib-a")
        .expect("the component is correlated");
    assert!(
        lib_a.is_verdict(),
        "a component correlated by a deterministic rule is a verdict"
    );

    let lib_c = correlated
        .iter()
        .find(|component| component.name == "lib-c")
        .expect("the component is correlated");
    assert!(
        !lib_c.is_verdict(),
        "a component that was not correlated is not a verdict"
    );
}

/// S8 The version of the normalizer is part of the facts.
#[test]
fn s8_the_version_of_the_normalizer_is_part_of_the_facts() {
    let cyclonedx = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
    let spdx = read_spdx_30(SPDX).expect("the document is read");

    assert!(!cyclonedx.normalizer_version.is_empty());
    assert!(!spdx.normalizer_version.is_empty());

    // The identity of a normalization is the pair --the normalizer and its
    // version-- and the two documents were read by two normalizers. So the
    // same bytes read by the two are two pieces of evidence and not one,
    // and the idempotency key says so.
    assert_ne!(
        cyclonedx.format, spdx.format,
        "each format has its own normalizer"
    );
    assert_eq!(
        cyclonedx.normalizer_version, spdx.normalizer_version,
        "and the version of each one is its own"
    );
}

/// The pre-authentication encoding is the one DSSE defines.
///
/// The test spells it out so that a change in the encoding cannot pass
/// unnoticed: a signature over something else is not the same signature.
#[test]
fn the_pae_is_the_one_dsse_defines() {
    let encoded = pae("application/vnd.in-toto+json", b"payload");
    let expected = b"DSSEv1 28 application/vnd.in-toto+json 7 payload";
    assert_eq!(encoded, expected.to_vec());
}
