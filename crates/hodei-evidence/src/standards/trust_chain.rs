//! Versioned trust roots, the chain verification they make possible, and
//! the offline snapshot that keeps crypto and freshness apart.
//!
//! M2 verified a DSSE signature against a flat key ring and said, in its
//! own docs, that it did **not** renew a trust root and did **not** talk
//! to a transparency log. This module is where that gap is closed --
//! offline and deterministically:
//!
//! - [`TrustRoots`] is the deployment's set of trust roots, organized as
//!   **lineages of numbered revisions**. A revision can be retired -- it
//!   stops signing, its past signatures still verify -- or revoked -- the
//!   policy takes its trust back, and a signature that cryptographically
//!   holds is refused with the reason `Revoked`. Rotations are recorded:
//!   who moved the lineage from which revision to which, when, and why.
//! - [`cosign_verify`] is the chain check a cosign-style verifier makes:
//!   the PAE signature against the key the bundle names, the key against
//!   the roots the deployment pinned, the statement against the artifact
//!   it is offered for, and the identity the statement and the root name
//!   against the identity expected. It **reuses** [`verify_envelope`] for
//!   the envelope, the PAE and the ECDSA check and the shared statement
//!   parser for the reading; it adds no second signature validator.
//! - [`OfflineSnapshot`] freezes a verification at a moment and lets it be
//!   read back later **without pretending to know today**: the signature
//!   either holds over the frozen bytes or it does not -- that is a fact
//!   about mathematics, recomputable offline forever -- while the
//!   freshness of the evidence *today* is a question an offline verifier
//!   cannot answer, and [`FreshnessNow::UnknownOffline`] is the honest
//!   value. A snapshot whose crypto holds and whose freshness is unknown
//!   is neither a current attestation nor a stale one.
//!
//! # What offline verification is not
//!
//! There is no transparency-log inclusion check and no live revocation
//! fetch here: offline, the deployment trusts the roots it shipped at
//! snapshot time and the revocations those roots carry. The freshness of
//! that trust today is reported as unknown, never guessed.

use crate::domain::capsule::DimensionState;
use crate::domain::content::ContentDigest;
use crate::domain::standards::attestation::{
    Attestation, AttestationStatement, AttestationVerification, DsseEnvelope,
    IN_TOTO_STATEMENT_TYPE, SignatureFailure,
};
use crate::standards::attestation::{AttestationError, parse_statement};
use crate::standards::dsse::{KeyRing, VerificationKey, verify_envelope};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The version of the trust-chain verifier.
pub const TRUST_CHAIN_VERSION: &str = "1.0.0";

/// One versioned revision of one trust root lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRevision {
    /// The lineage the revision belongs to -- the site, or root set,
    /// whose key it is. A deployment pins the lineage and the issuer
    /// together.
    pub lineage: String,
    /// The revision of the lineage, starting at 1.
    pub revision: u32,
    /// The key identifier the bundles name; the ring is keyed by it.
    pub key: VerificationKey,
    /// The issuer the root is pinned to, when the deployment pins one.
    pub issuer: Option<String>,
    /// When the revision started to be trusted.
    pub activated_at: DateTime<Utc>,
    /// When the revision stopped signing. A retired revision still
    /// verifies the signatures it made; retirement is not a failure.
    pub retired_at: Option<DateTime<Utc>>,
    /// When the policy took its trust back. A revoked revision does not
    /// vouch for anything anymore, even when its signature holds.
    pub revoked_at: Option<DateTime<Utc>>,
}

impl RootRevision {
    /// Whether the revision was revoked at or before the given moment.
    pub fn is_revoked_at(&self, at: DateTime<Utc>) -> bool {
        self.revoked_at.is_some_and(|when| when <= at)
    }

    /// Whether the revision was retired at or before the given moment.
    pub fn is_retired_at(&self, at: DateTime<Utc>) -> bool {
        self.retired_at.is_some_and(|when| when <= at)
    }
}

/// What a rotation did, recorded when it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotationRecord {
    /// The lineage that moved.
    pub lineage: String,
    /// The revision it moved away from, if it moved from one.
    pub from_revision: Option<u32>,
    /// The revision it moved to.
    pub to_revision: u32,
    /// Whether the old revision was revoked or merely retired. Revocation
    /// is what invalidates the past; retirement only stops the future.
    pub revoked_previous: bool,
    /// Who rotated.
    pub rotated_by: String,
    /// Why, in their words.
    pub reason: String,
    /// When.
    pub rotated_at: DateTime<Utc>,
}

/// The trust roots a deployment ships.
///
/// The roots are grouped by lineage and reachable by key id. Every
/// revision stays reachable -- revoked ones included -- so that a
/// verifier can tell the three failures apart: a key the deployment does
/// not know, a key it knows whose trust was taken back, and a key that
/// simply did not sign these bytes.
#[derive(Debug, Clone, Default)]
pub struct TrustRoots {
    revisions: BTreeMap<String, Vec<RootRevision>>,
    history: Vec<RotationRecord>,
}

impl TrustRoots {
    /// No roots at all: every bundle verifies against nobody.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current -- last activated -- revision of a lineage.
    pub fn current(&self, lineage: &str) -> Option<&RootRevision> {
        self.revisions.get(lineage).and_then(|v| v.last())
    }

    /// The revision a key id names, wherever it lives.
    pub fn find_key(&self, key_id: &str) -> Option<&RootRevision> {
        self.revisions
            .values()
            .flatten()
            .find(|revision| revision.key.key_id == key_id)
    }

    /// The ring of every revision, revoked ones included.
    ///
    /// The revoked key stays in the ring on purpose: the signature over a
    /// revoked key still *holds*, and it is the policy -- not the
    /// mathematics -- that refuses it. Leaving it out of the ring would
    /// hide a revocation behind an unknown key.
    pub fn key_ring(&self) -> KeyRing {
        let mut ring = KeyRing::new();
        for revision in self.revisions.values().flatten() {
            ring.add(revision.key.clone());
        }
        ring
    }

    /// Activates the first revision of a lineage, recording the act.
    pub fn activate(&mut self, revision: RootRevision, rotated_by: &str, reason: &str) {
        let activated_at = revision.activated_at;
        let lineage = revision.lineage.clone();
        let to = revision.revision;
        self.revisions
            .entry(lineage.clone())
            .or_default()
            .push(revision);
        self.history.push(RotationRecord {
            lineage,
            from_revision: None,
            to_revision: to,
            revoked_previous: false,
            rotated_by: rotated_by.to_string(),
            reason: reason.to_string(),
            rotated_at: activated_at,
        });
    }

    /// Rotates a lineage to a new revision, optionally revoking the one
    /// it replaces.
    ///
    /// The rotation **records** what it did and moves the *future* state
    /// of the lineage. It does not touch any decision made under the old
    /// revision: those decisions name the revision they were reached
    /// with, and the history keeps them readable forever. What happens to
    /// the grants that depended on the old revision is the reprocessing
    /// path -- the reevaluation chain of HAE-04, which writes a *new*
    /// decision and revokes the grants of the refused one -- and not this
    /// method.
    pub fn rotate(
        &mut self,
        lineage: &str,
        next: RootRevision,
        revoke_previous: bool,
        at: DateTime<Utc>,
        rotated_by: &str,
        reason: &str,
    ) -> Result<RotationRecord, TrustRootsError> {
        let previous = self
            .revisions
            .get(lineage)
            .and_then(|v| v.last().cloned())
            .ok_or_else(|| TrustRootsError::UnknownLineage(lineage.to_string()))?;

        if next.revision != previous.revision + 1 {
            return Err(TrustRootsError::NotTheNextRevision {
                lineage: lineage.to_string(),
                expected: previous.revision + 1,
                got: next.revision,
            });
        }

        let record = RotationRecord {
            lineage: lineage.to_string(),
            from_revision: Some(previous.revision),
            to_revision: next.revision,
            revoked_previous: revoke_previous,
            rotated_by: rotated_by.to_string(),
            reason: reason.to_string(),
            rotated_at: at,
        };

        // The old revision stops signing at the rotation; revocation is
        // the stronger move and it is what the policy reads.
        if let Some(entries) = self.revisions.get_mut(lineage)
            && let Some(old) = entries.last_mut()
        {
            old.retired_at = Some(at);
            if revoke_previous {
                old.revoked_at = Some(at);
            }
            entries.push(next);
        }
        self.history.push(record.clone());
        Ok(record)
    }

    /// The history of the activations and rotations, in order.
    pub fn history(&self) -> &[RotationRecord] {
        &self.history
    }

    /// How many lineages the deployment carries.
    pub fn lineages(&self) -> usize {
        self.revisions.len()
    }
}

/// Why a roots operation could not be done.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrustRootsError {
    /// The lineage is not one the deployment carries.
    #[error("there is no trust root lineage named {0}")]
    UnknownLineage(String),
    /// A rotation has to move a lineage one revision forward, not
    /// sideways.
    #[error("the lineage {lineage} is at revision {expected}, not {got}")]
    NotTheNextRevision {
        /// The lineage.
        lineage: String,
        /// The revision the rotation was expected to bring.
        expected: u32,
        /// The revision it brought.
        got: u32,
    },
}

/// What the chain verifier expects the bundle and the artifact to say.
///
/// The subject is the thing the attestation is offered for; the issuer
/// and the builder are the identities a trusted root is pinned to. A
/// `None` pin is not checked -- a root that pinned nothing cannot be
/// contradicted about what it did not pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedIdentity {
    /// The algorithm of the subject digest, e.g. `sha256`.
    pub subject_algorithm: String,
    /// The digest of the artifact the attestation must speak about.
    pub subject_digest: String,
    /// The issuer the signing root must be pinned to, when pinned.
    pub issuer: Option<String>,
    /// The builder identity the statement must name, when pinned.
    pub builder_id: Option<String>,
}

impl ExpectedIdentity {
    /// An expectation over a `sha256` digest and nothing pinned.
    pub fn of_digest(digest: impl Into<String>) -> Self {
        Self {
            subject_algorithm: "sha256".to_string(),
            subject_digest: digest.into(),
            issuer: None,
            builder_id: None,
        }
    }
}

/// What the chain verifier produced: the attestation with its
/// verification, and what the roots say about the key that signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainVerification {
    /// The attestation, with the verification the chain reached.
    pub attestation: Attestation,
    /// The lineage of the key that signed, when the key is one of the
    /// deployment's roots.
    pub root_lineage: Option<String>,
    /// The revision of that lineage.
    pub root_revision: Option<u32>,
    /// The key itself, when it is one of the roots: what an offline
    /// snapshot freezes so the signature can be re-checked later without
    /// the network.
    pub root_key: Option<VerificationKey>,
    /// Whether the PAE signature holds over the bytes, independently of
    /// what the policy decided afterwards. The distinction survives a
    /// revocation downgrade: the bytes *were* said by that key; the
    /// trust says it does not count anymore.
    pub cryptographic_holds: bool,
}

/// Verifies a bundle through the chain: envelope, PAE signature, root
/// policy, subject and pinned identity.
///
/// The checks happen in a fixed order and the first one that refuses
/// decides the verification, because each answers a different question:
///
/// 1. **The PAE signature** -- by [`verify_envelope`], the one signature
///    validator of the crate. A payload that holds a signature but cannot
///    be read as a statement is an **incomplete bundle**, not a bad
///    signature: `cryptographic_holds` keeps the verdict of the bytes.
/// 2. **The root policy** -- the signing key must be one of the pinned
///    roots and its revision not revoked *as of the verification moment*.
/// 3. **The subject** -- the statement must speak about the artifact the
///    verification is offered for.
/// 4. **The pinned identity** -- the issuer of the root and the builder
///    the statement names must be the ones the deployment expects. A
///    trusted key signing as somebody else is a foreign seat.
pub fn cosign_verify(
    envelope: &str,
    roots: &TrustRoots,
    expected: &ExpectedIdentity,
    at: DateTime<Utc>,
) -> Result<ChainVerification, AttestationError> {
    let envelope_value: Value = serde_json::from_str(envelope)
        .map_err(|error| AttestationError::MalformedJson(error.to_string()))?;
    let envelope_doc: DsseEnvelope = serde_json::from_value(envelope_value)
        .map_err(|error| AttestationError::MalformedEnvelope(error.to_string()))?;

    let source_digest = ContentDigest::of_bytes(envelope.as_bytes());
    let ring = roots.key_ring();
    let outcome = verify_envelope(&envelope_doc, &ring);
    let cryptographic_holds = outcome.verification.proves();

    // The payload is read through the one statement parser of the crate.
    // A signature over material that is not a readable statement is an
    // incomplete bundle -- the first refusal is the reading, and the
    // verdict of the bytes stays recorded in `cryptographic_holds`.
    let parsed: Result<Option<AttestationStatement>, AttestationVerification> =
        match &outcome.payload {
            Some(payload) => match serde_json::from_slice::<Value>(payload) {
                Ok(statement_value) => {
                    let statement_type = statement_value
                        .get("_type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if statement_type != IN_TOTO_STATEMENT_TYPE {
                        Ok(Some(AttestationStatement {
                            statement_type,
                            ..empty_statement()
                        }))
                    } else {
                        parse_statement(&statement_value)
                            .map(Some)
                            .map_err(|error| AttestationVerification::Invalid {
                                reason: SignatureFailure::IncompleteBundle(error.to_string()),
                            })
                    }
                }
                Err(error) => Err(AttestationVerification::Invalid {
                    reason: SignatureFailure::IncompleteBundle(error.to_string()),
                }),
            },
            None => Err(AttestationVerification::Invalid {
                reason: SignatureFailure::IncompleteBundle(
                    "the envelope carries no readable payload".to_string(),
                ),
            }),
        };
    let mut attestation = match parsed {
        Ok(Some(statement)) => Attestation {
            statement,
            verification: outcome.verification.clone(),
            source_digest,
        },
        Ok(None) => Attestation {
            statement: empty_statement(),
            verification: outcome.verification.clone(),
            source_digest,
        },
        Err(verification) => Attestation {
            statement: empty_statement(),
            verification,
            source_digest,
        },
    };

    // A statement whose type is not the one this version reads is a
    // refusal of the reading, whatever the signature says.
    if attestation.statement.statement_type != IN_TOTO_STATEMENT_TYPE
        && matches!(
            attestation.verification,
            AttestationVerification::Verified { .. }
        )
    {
        attestation.verification = AttestationVerification::Invalid {
            reason: SignatureFailure::UnsupportedStatement(
                attestation.statement.statement_type.clone(),
            ),
        };
    }

    let mut chain = ChainVerification {
        attestation,
        root_lineage: None,
        root_revision: None,
        root_key: None,
        cryptographic_holds,
    };

    // The chain continues only over a signature that holds and a bundle
    // that can be read.
    let AttestationVerification::Verified { key_id } = chain.attestation.verification.clone()
    else {
        // Even a refusal that is not "the bytes were said by a known
        // key" names its root when the key the bundle signs with is one
        // of the deployment's, so the refusal can be audited against the
        // lineage.
        let named = envelope_doc
            .signatures
            .first()
            .map(|signature| signature.keyid.clone())
            .unwrap_or_default();
        if let Some(revision) = roots.find_key(&named) {
            chain.root_lineage = Some(revision.lineage.clone());
            chain.root_revision = Some(revision.revision);
            chain.root_key = Some(revision.key.clone());
        }
        return Ok(chain);
    };

    let revision = roots
        .find_key(&key_id)
        .expect("the ring was built from the roots");
    chain.root_lineage = Some(revision.lineage.clone());
    chain.root_revision = Some(revision.revision);
    chain.root_key = Some(revision.key.clone());

    // 2. The root policy: a revoked revision is refused even though its
    // signature holds. The refusal is Invalid{Revoked}, never UnknownKey.
    if revision.is_revoked_at(at) {
        chain.attestation.verification = AttestationVerification::Invalid {
            reason: SignatureFailure::Revoked { key_id },
        };
        return Ok(chain);
    }

    // 3. The subject: the statement must speak about this artifact.
    if !chain
        .attestation
        .speaks_about(&expected.subject_algorithm, &expected.subject_digest)
    {
        chain.attestation.verification = AttestationVerification::Invalid {
            reason: SignatureFailure::SubjectMismatch {
                expected: format!("{}:{}", expected.subject_algorithm, expected.subject_digest),
            },
        };
        return Ok(chain);
    }

    // 4. The pinned identity: the issuer of the root and the builder of
    // the statement, when the deployment pinned either.
    if let Some(pinned) = &expected.issuer
        && revision.issuer.as_deref() != Some(pinned.as_str())
    {
        chain.attestation.verification = AttestationVerification::Invalid {
            reason: SignatureFailure::IdentityMismatch {
                expected: pinned.clone(),
                found: revision
                    .issuer
                    .clone()
                    .unwrap_or_else(|| "<none>".to_string()),
            },
        };
        return Ok(chain);
    }

    if let Some(pinned) = &expected.builder_id
        && !chain.attestation.statement.predicate.builder_id.is_empty()
        && chain.attestation.statement.predicate.builder_id != *pinned
    {
        chain.attestation.verification = AttestationVerification::Invalid {
            reason: SignatureFailure::IdentityMismatch {
                expected: pinned.clone(),
                found: chain.attestation.statement.predicate.builder_id.clone(),
            },
        };
    }

    Ok(chain)
}

/// A statement with nothing in it, for the bundles that could not be
/// read whole: the one empty statement of the crate, reused.
pub(crate) fn empty_statement() -> AttestationStatement {
    super::attestation::empty_statement()
}

/// How the freshness of a snapshot reads **today**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessNow {
    /// The freshness was evaluated online and the snapshot is within the
    /// threshold.
    Fresh,
    /// The freshness was evaluated online and the snapshot is past the
    /// threshold. This is a *known* bad freshness -- not the same thing
    /// as not knowing it at all.
    Stale,
    /// The verification was read offline: today's freshness is **not
    /// known**. Not fresh, not stale -- unmeasured, and unmeasured is
    /// never a green light.
    UnknownOffline,
}

impl FreshnessNow {
    /// The capsule state that says the same thing.
    ///
    /// The point is the third arm: an offline reading is not a stale
    /// one, and turning ignorance into `STALE` -- or, worse, into
    /// `FRESH` -- is exactly the confusion this type exists to prevent.
    pub fn dimension_state(&self) -> DimensionState {
        match self {
            Self::Fresh => DimensionState::Fresh,
            Self::Stale => DimensionState::Stale,
            Self::UnknownOffline => DimensionState::Unknown,
        }
    }
}

/// A verification frozen at a moment, kept whole enough to be read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineSnapshot {
    /// The envelope exactly as it was verified.
    pub envelope: String,
    /// The verification the chain reached **at the snapshot moment**,
    /// against the roots as they were then.
    pub verification: AttestationVerification,
    /// The lineage and revision the signature was checked against.
    pub root_lineage: Option<String>,
    /// The root revision frozen with the snapshot.
    pub root_revision: Option<u32>,
    /// The key as it was trusted at snapshot time. The frozen key is what
    /// makes a later offline re-check cryptographic: the bytes either
    /// match the key or they do not, and that needs no network.
    pub frozen_key: Option<VerificationKey>,
    /// The artifact the verification was about.
    pub artifact_digest: ContentDigest,
    /// When the snapshot was taken.
    pub taken_at: DateTime<Utc>,
}

/// What a snapshot reading says: the two facts the AC keeps apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotReading {
    /// Whether the signature holds over the bytes of the snapshot -- a
    /// fact about mathematics, true at the snapshot and recomputable
    /// offline forever.
    pub cryptographic_holds: bool,
    /// What is known about the freshness of the evidence *today*.
    pub freshness_now: FreshnessNow,
    /// The verification recorded at the snapshot moment.
    pub recorded_verification: AttestationVerification,
}

impl SnapshotReading {
    /// Whether the reading may be treated as a verified *current* piece
    /// of evidence.
    ///
    /// Only crypto **and** a known-fresh reading make a current proof.
    /// A snapshot whose crypto holds and whose freshness is unknown is
    /// evidence of a verification that happened -- and of nothing that
    /// is known now.
    pub fn proves_current(&self) -> bool {
        self.cryptographic_holds && self.freshness_now == FreshnessNow::Fresh
    }
}

impl OfflineSnapshot {
    /// Freezes a chain verification at the moment it was reached.
    pub fn freeze(
        verification: &ChainVerification,
        envelope: &str,
        taken_at: DateTime<Utc>,
    ) -> Self {
        let artifact = verification
            .attestation
            .statement
            .subject
            .first()
            .and_then(|subject| subject.digest("sha256"))
            .map(ContentDigest::sha256)
            .unwrap_or_else(|| verification.attestation.source_digest.clone());

        Self {
            envelope: envelope.to_string(),
            verification: verification.attestation.verification.clone(),
            root_lineage: verification.root_lineage.clone(),
            root_revision: verification.root_revision,
            frozen_key: verification.root_key.clone(),
            artifact_digest: artifact,
            taken_at,
        }
    }

    /// The reading of the snapshot made **offline**: the crypto is
    /// recomputed against the frozen key -- bytes need no network -- and
    /// today's freshness is declared unknown, never inferred from how
    /// old the snapshot is.
    pub fn review_offline(&self) -> SnapshotReading {
        SnapshotReading {
            cryptographic_holds: self.recheck_signature(),
            freshness_now: FreshnessNow::UnknownOffline,
            recorded_verification: self.verification.clone(),
        }
    }

    /// The reading of the snapshot made **online** at a moment, with the
    /// threshold the deployment uses. Contrast with
    /// [`review_offline`][OfflineSnapshot::review_offline]: here the
    /// freshness is a verdict, and past the threshold it is honestly
    /// `Stale`.
    pub fn review_online(&self, now: DateTime<Utc>, maximum_age: Duration) -> SnapshotReading {
        SnapshotReading {
            cryptographic_holds: self.recheck_signature(),
            freshness_now: if now.signed_duration_since(self.taken_at) > maximum_age {
                FreshnessNow::Stale
            } else {
                FreshnessNow::Fresh
            },
            recorded_verification: self.verification.clone(),
        }
    }

    /// Re-runs the PAE signature check against the frozen key.
    fn recheck_signature(&self) -> bool {
        let Some(key) = &self.frozen_key else {
            // Nothing was frozen: the chain never reached a key of the
            // deployment. There is no cryptography to claim.
            return false;
        };
        let Ok(envelope_doc) = serde_json::from_str::<DsseEnvelope>(&self.envelope) else {
            return false;
        };
        let outcome = verify_envelope(&envelope_doc, &KeyRing::with_key(key.clone()));
        outcome.verification.proves()
    }

    /// Whether the payload of the envelope still decodes: an incomplete
    /// bundle stays incomplete forever, network or not.
    pub fn payload_readable(&self) -> bool {
        serde_json::from_str::<DsseEnvelope>(&self.envelope)
            .ok()
            .and_then(|envelope| BASE64.decode(envelope.payload.as_bytes()).ok())
            .and_then(|payload| serde_json::from_slice::<Value>(&payload).ok())
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision(key_id: &str, lineage: &str, revision: u32) -> RootRevision {
        RootRevision {
            lineage: lineage.to_string(),
            revision,
            key: VerificationKey {
                key_id: key_id.to_string(),
                // Not a point: the unit tests below never verify bytes,
                // only the bookkeeping of the lineage.
                point: vec![0x04; 65],
            },
            issuer: None,
            activated_at: Utc::now(),
            retired_at: None,
            revoked_at: None,
        }
    }

    #[test]
    fn a_rotation_moves_the_lineage_one_revision_forward() {
        let mut roots = TrustRoots::new();
        roots.activate(revision("k1", "alpha", 1), "alice", "the first one");

        let record = roots
            .rotate(
                "alpha",
                revision("k2", "alpha", 2),
                true,
                Utc::now(),
                "bob",
                "the old key leaked",
            )
            .expect("the rotation is accepted");
        assert_eq!(record.from_revision, Some(1));
        assert_eq!(record.to_revision, 2);
        assert!(record.revoked_previous);

        // The old revision is revoked but still reachable -- its past
        // signatures can still be identified -- and the new one is
        // current.
        assert!(
            roots
                .find_key("k1")
                .expect("k1 stays")
                .is_revoked_at(Utc::now())
        );
        assert_eq!(roots.current("alpha").expect("a current").key.key_id, "k2");
    }

    #[test]
    fn a_rotation_cannot_skip_or_repeat_a_revision() {
        let mut roots = TrustRoots::new();
        roots.activate(revision("k1", "alpha", 1), "alice", "the first one");

        let sideways = roots.rotate(
            "alpha",
            revision("k3", "alpha", 3),
            false,
            Utc::now(),
            "bob",
            "skip",
        );
        assert_eq!(
            sideways,
            Err(TrustRootsError::NotTheNextRevision {
                lineage: "alpha".to_string(),
                expected: 2,
                got: 3
            })
        );
    }
}
