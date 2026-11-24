//! The contract of the trust chain: versioned roots, the cosign-style
//! verification, and the offline snapshot.
//!
//! These are the conformance fixtures of HAE-21. They read the real
//! bundles of `fixtures/trust-chain/` -- signed offline with throwaway
//! keys derived from fixed labels -- and they say what the product may
//! rely on when a signature, a root and an identity have to be read as
//! one chain.
//!
//! # Guarantees
//!
//! - C1 A bundle verifies only against the root the deployment pins: the
//!   signature, the key, the lineage and the revision are one answer.
//! - C2 A wrong digest is refused with the reason, and the bytes still
//!   say what they said: `cryptographic_holds` and the verdict are kept
//!   apart.
//! - C3 A signature made by a foreign root is not made trustworthy by
//!   the pinning: an issuer pin against another issuer is an identity
//!   mismatch, not a signature failure.
//! - C4 A revoked root refuses a signature that cryptographically holds.
//!   `Revoked` is not `UnknownKey`: the bytes *were* said by a key the
//!   deployment carried, and the policy took that trust back.
//! - C5 A bundle that is signed but carries no readable statement is an
//!   incomplete bundle, and the signature that holds over it is recorded.
//! - C6 The offline snapshot keeps a cryptographic fact of the snapshot
//!   apart from a freshness that is not known today: the crypto is
//!   recomputable offline; the freshness is never guessed.

use chrono::{DateTime, Duration, Utc};
use hodei_evidence::domain::standards::attestation::{AttestationVerification, SignatureFailure};
use hodei_evidence::standards::dsse::VerificationKey;
use hodei_evidence::standards::{
    ExpectedIdentity, FreshnessNow, OfflineSnapshot, RootRevision, TRUST_CHAIN_VERSION, TrustRoots,
    cosign_verify,
};

/// The bundle the alpha root signed over the good subject.
const GOOD: &str = include_str!("fixtures/trust-chain/cosign-good.dsse.json");

/// The same subject signed by the foreign beta root.
const FOREIGN_ROOT: &str = include_str!("fixtures/trust-chain/cosign-foreign-root.dsse.json");

/// A bundle whose statement names a different digest.
const WRONG_DIGEST: &str = include_str!("fixtures/trust-chain/cosign-wrong-digest.dsse.json");

/// A bundle whose signature holds over a statement that is not readable.
const TRUNCATED: &str = include_str!("fixtures/trust-chain/cosign-truncated.dsse.json");

/// A bundle with no signatures at all.
const BARE: &str = include_str!("fixtures/trust-chain/cosign-bare.dsse.json");

/// The raw public point of the alpha root.
const ALPHA_POINT: &str = include_str!("fixtures/trust-chain/trust-root-alpha-public-point.hex");

/// The raw public point of the foreign beta root.
const BETA_POINT: &str = include_str!("fixtures/trust-chain/trust-root-beta-public-point.hex");

/// The key identifier the alpha bundles name.
const KEY_ID_ALPHA: &str = "hsp154-root-alpha-v1";

/// The key identifier the foreign bundle names.
const KEY_ID_BETA: &str = "hsp154-foreign-beta-v1";

/// The digest the good bundles speak about.
const GOOD_DIGEST: &str = "ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06";

/// The builder the good statements name.
const BUILDER: &str = "https://builders.example/hsp154-builder@v1";

/// Decodes one of the hexadecimal points of the fixtures.
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

/// A fixed moment: the build finished at 2026-03-04T10:12Z, the snapshot
/// was taken at 10:15Z.
fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("a timestamp")
        .with_timezone(&Utc)
}

/// One revision of the alpha lineage, trusted from the epoch.
fn alpha_revision(issuer: Option<&str>, revoked_at: Option<DateTime<Utc>>) -> RootRevision {
    RootRevision {
        lineage: "alpha".to_string(),
        revision: 1,
        key: VerificationKey {
            key_id: KEY_ID_ALPHA.to_string(),
            point: point(ALPHA_POINT),
        },
        issuer: issuer.map(str::to_string),
        activated_at: at("2026-01-01T00:00:00Z"),
        retired_at: None,
        revoked_at,
    }
}

/// The deployment's roots: the alpha lineage only.
fn alpha_roots() -> TrustRoots {
    let mut roots = TrustRoots::new();
    roots.activate(
        alpha_revision(Some("registry.example"), None),
        "alice",
        "the root the deployment ships",
    );
    roots
}

/// The expectation of the good bundles: the good digest, the issuer and
/// the builder pinned.
fn expected_good() -> ExpectedIdentity {
    ExpectedIdentity {
        subject_algorithm: "sha256".to_string(),
        subject_digest: GOOD_DIGEST.to_string(),
        issuer: Some("registry.example".to_string()),
        builder_id: Some(BUILDER.to_string()),
    }
}

/// C1 The good bundle verifies against the pinned root, and the chain
/// names the lineage and the revision it was checked against.
#[test]
fn c1_a_good_bundle_verifies_against_the_pinned_root() {
    let chain = cosign_verify(
        GOOD,
        &alpha_roots(),
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Verified { key_id } if key_id == KEY_ID_ALPHA
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(chain.cryptographic_holds, "the bytes were said by the key");
    assert_eq!(chain.root_lineage.as_deref(), Some("alpha"));
    assert_eq!(chain.root_revision, Some(1));
    assert_eq!(
        chain.root_key.as_ref().map(|key| key.key_id.as_str()),
        Some(KEY_ID_ALPHA)
    );
    assert_eq!(chain.attestation.statement.predicate.builder_id, BUILDER);
    // The version of the verifier is part of the answer it gives.
    assert_eq!(TRUST_CHAIN_VERSION, "1.0.0");
}

/// C1 An empty ring is a failure of the trust, not of the signature.
#[test]
fn c1_without_the_pinned_root_the_key_is_unknown() {
    let chain = cosign_verify(
        GOOD,
        &TrustRoots::new(),
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Unverified {
                reason: SignatureFailure::UnknownKey(key_id)
            } if key_id == KEY_ID_ALPHA
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(
        !chain.cryptographic_holds,
        "nothing was verified against a key the deployment carries"
    );
}

/// C2 A wrong digest is refused with the reason, and the signature that
/// holds over it is recorded apart.
#[test]
fn c2_a_wrong_digest_is_a_subject_mismatch_not_a_bad_signature() {
    let chain = cosign_verify(
        WRONG_DIGEST,
        &alpha_roots(),
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::SubjectMismatch { expected }
            } if expected.as_str() == format!("sha256:{GOOD_DIGEST}")
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    // The bytes *were* said by the alpha key; the statement is about the
    // wrong artifact. The two facts are kept apart on purpose.
    assert!(
        chain.cryptographic_holds,
        "the signature holds even though the subject does not match"
    );
    assert_eq!(chain.root_lineage.as_deref(), Some("alpha"));
}

/// C3 A foreign root does not become one's own by pinning: the issuer of
/// the signing revision is checked against the pin.
#[test]
fn c3_a_foreign_issuer_is_an_identity_mismatch() {
    let mut roots = alpha_roots();
    // The deployment also carries the foreign lineage -- its key is known
    // and its signature holds -- but it is pinned to a different issuer.
    roots.activate(
        RootRevision {
            lineage: "beta".to_string(),
            revision: 1,
            key: VerificationKey {
                key_id: KEY_ID_BETA.to_string(),
                point: point(BETA_POINT),
            },
            issuer: Some("other-registry.example".to_string()),
            activated_at: at("2026-01-01T00:00:00Z"),
            retired_at: None,
            revoked_at: None,
        },
        "mallory",
        "a lineage of another site",
    );

    let chain = cosign_verify(
        FOREIGN_ROOT,
        &roots,
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::IdentityMismatch { expected, found }
            } if expected == "registry.example" && found == "other-registry.example"
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(
        chain.cryptographic_holds,
        "the foreign bytes are foreign bytes, not broken bytes"
    );
    assert_eq!(chain.root_lineage.as_deref(), Some("beta"));
}

/// C4 A revoked root refuses a signature that cryptographically holds.
#[test]
fn c4_a_revoked_root_is_refused_though_the_signature_holds() {
    let mut roots = TrustRoots::new();
    roots.activate(
        alpha_revision(Some("registry.example"), Some(at("2026-02-01T00:00:00Z"))),
        "alice",
        "the root that leaked",
    );

    let chain = cosign_verify(GOOD, &roots, &expected_good(), at("2026-03-04T10:15:00Z"))
        .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::Revoked { key_id }
            } if key_id == KEY_ID_ALPHA
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(
        chain.cryptographic_holds,
        "the signature holds; it is the trust that was taken back"
    );
    // The refusal still names the root: an auditable "no" knows what it
    // said no to.
    assert_eq!(chain.root_lineage.as_deref(), Some("alpha"));
    assert_eq!(chain.root_revision, Some(1));

    // The same key, verified at a moment BEFORE the revocation, still
    // holds: revocation is a fact about time, not a rewriting of it.
    let before = cosign_verify(GOOD, &roots, &expected_good(), at("2026-01-15T00:00:00Z"))
        .expect("the chain is read");
    assert!(
        matches!(
            before.attestation.verification,
            AttestationVerification::Verified { .. }
        ),
        "the revocation of 2026-02-01 cannot reach back to 2026-01-15: {}",
        before.attestation.verification
    );
}

/// C5 A signed bundle without a readable statement is incomplete, and
/// the signature that holds over the unreadable bytes is recorded.
#[test]
fn c5_a_signed_bundle_that_cannot_be_read_is_incomplete() {
    let chain = cosign_verify(
        TRUNCATED,
        &alpha_roots(),
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::IncompleteBundle(_)
            }
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(
        chain.cryptographic_holds,
        "the alpha key did say these bytes; the bundle is simply not whole"
    );
    // The statement was not read, so the chain cannot vouch for its
    // subject or its builder.
    assert!(
        chain.attestation.statement.subject.is_empty(),
        "an incomplete bundle has no readable subject"
    );
}

/// C5 A bare bundle is unverified, and it is not an invalid signature:
/// nothing was signed at all.
#[test]
fn c5_a_bundle_with_no_signatures_is_unverified() {
    let chain = cosign_verify(
        BARE,
        &alpha_roots(),
        &expected_good(),
        at("2026-03-04T10:15:00Z"),
    )
    .expect("the chain is read");

    assert!(
        matches!(
            &chain.attestation.verification,
            AttestationVerification::Unverified {
                reason: SignatureFailure::NoSignature
            }
        ),
        "the verdict: {}",
        chain.attestation.verification
    );
    assert!(!chain.cryptographic_holds);
}

/// C6 The offline snapshot: the crypto of the snapshot is recomputable
/// offline and today's freshness is declared unknown, never guessed.
#[test]
fn c6_an_offline_reading_keeps_the_crypto_apart_from_the_freshness() {
    let taken_at = at("2026-03-04T10:15:00Z");
    let chain = cosign_verify(GOOD, &alpha_roots(), &expected_good(), taken_at)
        .expect("the chain verifies at the snapshot moment");
    let snapshot = OfflineSnapshot::freeze(&chain, GOOD, taken_at);

    // Read today, without a network: the signature over the frozen bytes
    // still holds -- mathematics needs no connectivity -- and the
    // freshness of the evidence *now* is not known.
    let offline = snapshot.review_offline();
    assert!(
        offline.cryptographic_holds,
        "the crypto of the snapshot is a fact about its bytes"
    );
    assert_eq!(offline.freshness_now, FreshnessNow::UnknownOffline);
    assert!(
        !offline.proves_current(),
        "a valid signature at the snapshot is not a fresh verdict today"
    );
    assert_eq!(
        offline.recorded_verification, chain.attestation.verification,
        "the verification of the snapshot moment travels with it"
    );

    // The same snapshot read online: within the threshold it is fresh,
    // past it it is stale. The offline reading never infers either.
    let fresh = snapshot.review_online(taken_at + Duration::hours(1), Duration::hours(2));
    assert!(fresh.cryptographic_holds);
    assert_eq!(fresh.freshness_now, FreshnessNow::Fresh);
    assert!(fresh.proves_current());

    let stale = snapshot.review_online(taken_at + Duration::hours(3), Duration::hours(2));
    assert!(
        stale.cryptographic_holds,
        "staleness does not undo the signature: it says the verdict is old"
    );
    assert_eq!(stale.freshness_now, FreshnessNow::Stale);
    assert!(
        !stale.proves_current(),
        "a known-stale reading is not a current proof"
    );
}

/// C6 An offline reading does not turn into a false green light when the
/// root is revoked later: the frozen crypto still holds over the frozen
/// bytes, and the freshness stays unknown.
#[test]
fn c6_a_later_revocation_does_not_rewrite_the_frozen_crypto() {
    let taken_at = at("2026-03-04T10:15:00Z");
    let chain = cosign_verify(GOOD, &alpha_roots(), &expected_good(), taken_at)
        .expect("the chain verifies at the snapshot moment");
    let snapshot = OfflineSnapshot::freeze(&chain, GOOD, taken_at);

    // The deployment revokes the root afterwards.
    let mut revoked = TrustRoots::new();
    revoked.activate(
        alpha_revision(Some("registry.example"), Some(at("2026-03-05T00:00:00Z"))),
        "alice",
        "the root leaked",
    );
    let refusal = cosign_verify(GOOD, &revoked, &expected_good(), at("2026-03-06T00:00:00Z"))
        .expect("the chain is read");
    assert!(
        matches!(
            &refusal.attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::Revoked { .. }
            }
        ),
        "today's online verdict on the same bundle: {}",
        refusal.attestation.verification
    );

    // The offline reading of the snapshot says what the snapshot was: a
    // signature that held at the moment it was taken. It does not know
    // -- and does not pretend to know -- that the trust was taken back.
    let offline = snapshot.review_offline();
    assert!(offline.cryptographic_holds);
    assert_eq!(offline.freshness_now, FreshnessNow::UnknownOffline);
    assert!(!offline.proves_current());
}
