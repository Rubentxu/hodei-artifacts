//! The envelope that a producer delivers, and what Hodei makes of it.
//!
//! The acceptance pipeline of the plan is:
//!
//! ```text
//! receive -> authenticate producer -> validate envelope/schema ->
//! calculate content digest -> deduplicate/idempotency check ->
//! preserve raw evidence -> normalize deterministically ->
//! validate canonical identities -> accept/reject -> append accepted event
//! ```
//!
//! The envelope is the first thing in that line: what a producer delivers.
//! It carries the producer, the account it speaks for, the subjects it says
//! something about, the kind of evidence, the content it refers to, and the
//! signature that says it really comes from whom it says.
//!
//! # The canonical form
//!
//! The signature is over a canonical form: a byte sequence that depends only
//! on the contents of the envelope. Two producers that send the same thing
//! have to produce the same bytes, or the signature proves nothing.
//!
//! The encoding is length-prefixed on purpose. A separator would be shorter,
//! but a value that happens to contain the separator would make two
//! different envelopes produce the same bytes, and then the signature would
//! authenticate something nobody sent.

use crate::domain::content::ContentDigest;
use crate::domain::evidence::{
    EvidenceId, EvidenceType, EvidenceVerification, Normalization, Producer, SourceRef,
};
use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The envelope of an incoming piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionEnvelope {
    /// The producer that delivers the evidence.
    pub producer: Producer,
    /// The account the evidence belongs to.
    pub tenant_hrn: Hrn,
    /// The subjects the evidence says something about.
    #[serde(default)]
    pub subject_refs: Vec<Hrn>,
    /// The kind of evidence.
    pub evidence_type: EvidenceType,
    /// The schema the evidence follows, when it follows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_uri: Option<String>,
    /// The version of that schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
    /// Where the evidence came from, outside Hodei.
    pub source: SourceRef,
    /// The digest of the content.
    pub content_digest: ContentDigest,
    /// When the producer observed the evidence.
    pub observed_at: DateTime<Utc>,
    /// When the source says it became true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<DateTime<Utc>>,
    /// How far the verification of the evidence goes.
    pub verification: EvidenceVerification,
    /// How the evidence was normalized.
    pub normalization: Normalization,
    /// The correlation of the request that brought it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// The signature of the producer over the canonical form.
    pub signature: String,
}

impl IngestionEnvelope {
    /// The canonical form of the envelope: the bytes the signature is over.
    ///
    /// The encoding is length-prefixed: each field is written as its length,
    /// a colon, and its bytes. Two envelopes with the same contents produce
    /// the same bytes, and two envelopes with different contents cannot
    /// produce the same bytes.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut write = |name: &str, value: &[u8]| {
            bytes.extend_from_slice(name.len().to_string().as_bytes());
            bytes.push(b':');
            bytes.extend_from_slice(value);
            bytes.push(b'\n');
        };

        write("producer.kind", self.producer.kind.as_bytes());
        write("producer.id", self.producer.id.as_bytes());
        write(
            "producer.version",
            self.producer.version.as_deref().unwrap_or("").as_bytes(),
        );
        let tenant = self.tenant_hrn.to_string();
        write("tenant", tenant.as_bytes());
        let subjects = self.subject_refs.len().to_string();
        write("subjects", subjects.as_bytes());
        for subject in &self.subject_refs {
            let subject = subject.to_string();
            write("subject", subject.as_bytes());
        }
        let evidence_type = self.evidence_type.to_string();
        write("evidence_type", evidence_type.as_bytes());
        write(
            "schema_uri",
            self.schema_uri.as_deref().unwrap_or("").as_bytes(),
        );
        write(
            "schema_version",
            self.schema_version.as_deref().unwrap_or("").as_bytes(),
        );
        write(
            "source.external_id",
            self.source.external_id.as_deref().unwrap_or("").as_bytes(),
        );
        write(
            "source.uri",
            self.source.uri.as_deref().unwrap_or("").as_bytes(),
        );
        write("content_digest", self.content_digest.to_string().as_bytes());
        write("observed_at", self.observed_at.to_rfc3339().as_bytes());
        write(
            "valid_from",
            self.valid_from
                .map(|moment| moment.to_rfc3339())
                .unwrap_or_default()
                .as_bytes(),
        );
        write(
            "verification.status",
            self.verification.status.to_string().as_bytes(),
        );
        write(
            "verification.method",
            self.verification.method.as_deref().unwrap_or("").as_bytes(),
        );
        write(
            "normalization.normalizer",
            self.normalization.normalizer.as_bytes(),
        );
        write(
            "normalization.version",
            self.normalization.version.as_bytes(),
        );
        write(
            "correlation_id",
            self.correlation_id.as_deref().unwrap_or("").as_bytes(),
        );
        bytes
    }
}

/// Why a piece of evidence was not accepted.
///
/// The reasons are a closed set on purpose: a rejection that says "it did
/// not work" teaches the producer nothing, and a rejection that says which
/// step failed teaches it exactly what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RejectionReason {
    /// The producer could not be authenticated.
    #[error("the producer could not be authenticated")]
    Unauthenticated,
    /// The envelope is not coherent.
    #[error("the envelope is not valid: {0}")]
    InvalidEnvelope(String),
    /// The content does not have the digest that was declared.
    #[error("the content does not have the declared digest")]
    ContentDigestMismatch,
    /// The canonical identities of the envelope are not usable.
    #[error("the canonical identities are not valid: {0}")]
    InvalidIdentities(String),
    /// The store was not available.
    #[error("the store is not available")]
    Unavailable,
}

/// The outcome of an ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestionOutcome {
    /// The evidence was accepted for the first time.
    Accepted {
        /// The identity of the accepted record.
        evidence_id: EvidenceId,
        /// The identity of the event that was appended.
        event_id: crate::domain::event::EventId,
    },
    /// The evidence was already accepted: this delivery is a repetition.
    ///
    /// The record that was accepted the first time is returned, and nothing
    /// was written.
    AlreadyAccepted {
        /// The identity of the record that already existed.
        evidence_id: EvidenceId,
    },
    /// The evidence is a new revision of an external identity that already
    /// had one.
    ///
    /// The previous revision is kept: the history is not overwritten.
    Revised {
        /// The identity of the new record.
        evidence_id: EvidenceId,
        /// The identity of the record it supersedes.
        supersedes: EvidenceId,
        /// The identity of the event that was appended.
        event_id: crate::domain::event::EventId,
    },
    /// The evidence was refused.
    Rejected {
        /// Why it was refused.
        reason: RejectionReason,
    },
}

impl IngestionOutcome {
    /// The identity of the accepted record, when there is one.
    pub fn evidence_id(&self) -> Option<&EvidenceId> {
        match self {
            Self::Accepted { evidence_id, .. }
            | Self::AlreadyAccepted { evidence_id }
            | Self::Revised { evidence_id, .. } => Some(evidence_id),
            Self::Rejected { .. } => None,
        }
    }

    /// Whether the evidence was accepted, in any of its forms.
    pub fn is_accepted(&self) -> bool {
        !matches!(self, Self::Rejected { .. })
    }
}
