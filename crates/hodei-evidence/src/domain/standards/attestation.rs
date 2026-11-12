//! The attestation: what somebody claims about how a thing was built, and
//! whether the claim holds.
//!
//! The difference between a declaration and a proof is the whole point of
//! this part of the domain. An attestation that nobody checked says what
//! its author wanted to say; an attestation whose signature was verified
//! against the key of its issuer says what its issuer said. Both are kept,
//! and the difference between them is a value that a policy reads and that
//! a trace shows.
//!
//! # The layers
//!
//! The in-toto framework names them: a **predicate** says the thing, a
//! **statement** binds the predicate to the subjects it is about, an
//! **envelope** carries the statement with the signatures over it, and a
//! **bundle** carries the envelopes. Hodei reads the first three.
//!
//! # The DSSE envelope
//!
//! The envelope is the one Sigstore and in-toto use: a payload type, a
//! payload and the signatures over it. The signatures are over the **PAE**
//! --the pre-authentication encoding-- and not over the payload bytes, so
//! that the payload type is part of what is signed. A signature over the
//! raw bytes would prove something weaker.

use crate::domain::content::ContentDigest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The payload type of a DSSE envelope that carries an in-toto statement.
pub const IN_TOTO_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";

/// The type of an in-toto statement, version 1.
pub const IN_TOTO_STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";

/// The predicate type of an SLSA provenance, version 1.
pub const SLSA_PROVENANCE_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";

/// A DSSE envelope: a payload and the signatures over it.
///
/// The field names are the ones DSSE uses: `payloadType`, `payload` and
/// `signatures`, with `keyid` and `sig` inside a signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DsseEnvelope {
    /// The type of the payload.
    pub payload_type: String,
    /// The payload, base64 encoded.
    pub payload: String,
    /// The signatures over the payload.
    #[serde(default)]
    pub signatures: Vec<DsseSignature>,
}

/// A signature of a DSSE envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DsseSignature {
    /// The identifier of the key that signed.
    pub keyid: String,
    /// The signature, base64 encoded.
    pub sig: String,
}

/// A subject of an attestation: a thing, and the digests that identify it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationSubject {
    /// The name of the subject.
    pub name: String,
    /// The digests of the subject, by algorithm.
    #[serde(default)]
    pub digests: BTreeMap<String, String>,
}

impl AttestationSubject {
    /// The digest of the subject under an algorithm.
    pub fn digest(&self, algorithm: &str) -> Option<&str> {
        self.digests.get(algorithm).map(String::as_str)
    }
}

/// The predicate of an attestation, in the shape SLSA v1.2 gives it.
///
/// The predicate is kept as the document wrote it, and the facts Hodei
/// asks about are read out of it. Keeping it whole is what makes the
/// attestation inspectable later: the raw verification evidence stays
/// visible, as the M2 UAT requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlsaProvenance {
    /// The type of the build.
    #[serde(default)]
    pub build_type: String,
    /// The identifier of the builder that ran it.
    #[serde(default)]
    pub builder_id: String,
    /// The identifier of the invocation.
    #[serde(default)]
    pub invocation_id: String,
    /// When the build started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_on: Option<String>,
    /// When the build finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_on: Option<String>,
    /// The dependencies the build resolved.
    #[serde(default)]
    pub resolved_dependencies: Vec<ResolvedDependency>,
}

/// A dependency a build resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDependency {
    /// The URI of the dependency.
    #[serde(default)]
    pub uri: String,
    /// The digests of the dependency, by algorithm.
    #[serde(default)]
    pub digests: BTreeMap<String, String>,
}

/// An in-toto statement: a predicate bound to the subjects it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationStatement {
    /// The type of the statement.
    #[serde(rename = "_type", default)]
    pub statement_type: String,
    /// The type of the predicate.
    #[serde(rename = "predicateType", default)]
    pub predicate_type: String,
    /// The subjects the predicate is about.
    #[serde(default)]
    pub subject: Vec<AttestationSubject>,
    /// The predicate, in the shape SLSA v1.2 gives it.
    pub predicate: SlsaProvenance,
}

/// Why a signature could not be verified.
///
/// The reasons are a closed set, because a rejection that says "it did not
/// work" teaches the producer nothing. Every reason says what to look at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SignatureFailure {
    /// The envelope is not a DSSE envelope.
    #[error("the envelope is not a DSSE envelope: {0}")]
    MalformedEnvelope(String),
    /// The payload is not base64, or is not what it claims to be.
    #[error("the payload of the envelope could not be read: {0}")]
    MalformedPayload(String),
    /// The envelope carries no signature.
    #[error("the envelope carries no signature")]
    NoSignature,
    /// The key that signed is not one Hodei knows.
    #[error("the key that signed is not one this deployment knows: {0}")]
    UnknownKey(String),
    /// The signature is not in the form the algorithm needs.
    #[error("the signature is malformed: {0}")]
    MalformedSignature(String),
    /// The signature does not match the payload.
    #[error("the signature does not match the payload it claims to sign")]
    SignatureDoesNotMatch,
    /// The statement is not the kind of statement Hodei reads.
    #[error("the statement is not one this version reads: {0}")]
    UnsupportedStatement(String),
    /// The attestation does not speak about the thing it was offered for.
    #[error("the attestation does not speak about {expected}")]
    SubjectMismatch {
        /// The subject the attestation was expected to speak about.
        expected: String,
    },
}

/// How far the verification of an attestation goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestationVerification {
    /// The signature was checked against the key of its issuer, and it
    /// holds.
    Verified {
        /// The identifier of the key that signed.
        key_id: String,
    },
    /// The attestation was checked and it does not hold.
    Invalid {
        /// Why it does not hold.
        reason: SignatureFailure,
    },
    /// The attestation was not checked at all.
    ///
    /// An attestation that nobody checked is a declaration, and this is the
    /// value that says so.
    Unverified {
        /// Why it was not checked.
        reason: SignatureFailure,
    },
}

impl AttestationVerification {
    /// Whether the attestation is a proof.
    ///
    /// Only a verified attestation is a proof. An unverified one is a
    /// declaration and an invalid one is a contradiction.
    pub fn proves(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }

    /// The reason the attestation is not a proof, when it is not one.
    pub fn reason(&self) -> Option<&SignatureFailure> {
        match self {
            Self::Verified { .. } => None,
            Self::Invalid { reason } | Self::Unverified { reason } => Some(reason),
        }
    }
}

/// An attestation: a statement, and how far its verification goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    /// The statement the attestation carries.
    pub statement: AttestationStatement,
    /// How far the verification goes.
    pub verification: AttestationVerification,
    /// The digest of the document the statement came from.
    pub source_digest: ContentDigest,
}

impl Attestation {
    /// The digest of the subject the attestation speaks about, under an
    /// algorithm.
    pub fn subject_digest(&self, algorithm: &str) -> Option<&str> {
        self.statement
            .subject
            .iter()
            .find_map(|subject| subject.digest(algorithm))
    }

    /// Whether the attestation speaks about the given digest.
    pub fn speaks_about(&self, algorithm: &str, digest: &str) -> bool {
        self.statement
            .subject
            .iter()
            .any(|subject| subject.digest(algorithm) == Some(digest))
    }
}

impl fmt::Display for AttestationVerification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified { key_id } => write!(f, "VERIFIED by {key_id}"),
            Self::Invalid { reason } => write!(f, "INVALID: {reason}"),
            Self::Unverified { reason } => write!(f, "UNVERIFIED: {reason}"),
        }
    }
}
