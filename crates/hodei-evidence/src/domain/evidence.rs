//! The record of an accepted piece of evidence.
//!
//! An evidence record is the answer to "why does Hodei believe this?". It
//! says what the evidence is, who produced it, what it says about which
//! subjects, what its content digest is, and how far its verification goes.
//!
//! Two rules shape the whole contract:
//!
//! - **The record references the content, it does not carry it.** The payload
//!   stays where the producer put it; the record keeps the digest, the
//!   location and the facts about it. A record therefore cannot hold a
//!   secret, because it has no field where a secret would go.
//! - **The temporal facts are kept apart.** `observed_at` is when Hodei
//!   learned it, `valid_from` is when the source says it became true, and
//!   `recorded_at` is when it was accepted. Collapsing them into one
//!   timestamp makes bitemporal questions unanswerable later.

use crate::domain::content::{ContentError, ContentRef};
use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The identity of an accepted piece of evidence.
///
/// The identifier is derived, not generated: the same evidence accepted
/// twice has the same identity, which is what makes the acceptance
/// idempotent without a table of lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EvidenceId(String);

impl EvidenceId {
    /// The identifier of an evidence record.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The kind of evidence.
///
/// The list is the initial one of the plan. It is not closed forever: a new
/// kind of evidence is a new value, and the reader that meets an unknown one
/// says so rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    /// Metadata about the source: the repository, the branch, the commit.
    SourceMetadata,
    /// Metadata about the build: the pipeline, the runner, the environment.
    BuildMetadata,
    /// The digest and the integrity of an artifact.
    ArtifactIntegrity,
    /// A software bill of materials.
    Sbom,
    /// A provenance or attestation document.
    Provenance,
    /// The verification of a signature.
    SignatureVerification,
    /// The result of a test.
    TestResult,
    /// The result of a static or dynamic analysis.
    ScanResult,
    /// A secret found by a scanner.
    SecretFinding,
    /// A finding about infrastructure as code.
    IaCFinding,
    /// Intelligence about a vulnerability.
    VulnerabilityIntelligence,
    /// A statement that a vulnerability does or does not affect a product.
    Vex,
    /// The observation of a deployment.
    DeploymentObservation,
    /// The exposure of a workload at runtime.
    RuntimeExposure,
    /// A human approval or exception.
    HumanApproval,
    /// A claim produced by a model.
    AiClaim,
    /// A kind of evidence that this version does not know.
    #[serde(untagged)]
    Other(String),
}

impl fmt::Display for EvidenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMetadata => f.write_str("source_metadata"),
            Self::BuildMetadata => f.write_str("build_metadata"),
            Self::ArtifactIntegrity => f.write_str("artifact_integrity"),
            Self::Sbom => f.write_str("sbom"),
            Self::Provenance => f.write_str("provenance"),
            Self::SignatureVerification => f.write_str("signature_verification"),
            Self::TestResult => f.write_str("test_result"),
            Self::ScanResult => f.write_str("scan_result"),
            Self::SecretFinding => f.write_str("secret_finding"),
            Self::IaCFinding => f.write_str("iac_finding"),
            Self::VulnerabilityIntelligence => f.write_str("vulnerability_intelligence"),
            Self::Vex => f.write_str("vex"),
            Self::DeploymentObservation => f.write_str("deployment_observation"),
            Self::RuntimeExposure => f.write_str("runtime_exposure"),
            Self::HumanApproval => f.write_str("human_approval"),
            Self::AiClaim => f.write_str("ai_claim"),
            Self::Other(value) => f.write_str(value),
        }
    }
}

/// Who produced the evidence.
///
/// The producer is identified, not anonymous: an evidence without a producer
/// cannot be weighed against another one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// The kind of producer: a connector, a person, a service.
    pub kind: String,
    /// The identity of the producer.
    pub id: String,
    /// The version of the producer, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Producer {
    /// A producer of a given kind and identity.
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            version: None,
        }
    }

    /// The producer, with its version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

/// Where the evidence came from, outside Hodei.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    /// The identifier the producer gave to the evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Where the evidence can be found, when it lives somewhere known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

impl SourceRef {
    /// A source with no external reference at all.
    pub fn none() -> Self {
        Self {
            external_id: None,
            uri: None,
        }
    }

    /// A source identified by the producer's own identifier.
    pub fn by_external_id(external_id: impl Into<String>) -> Self {
        Self {
            external_id: Some(external_id.into()),
            uri: None,
        }
    }
}

/// How far the verification of the evidence goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum VerificationStatus {
    /// The evidence was verified: its signature or its digest was checked
    /// against something trusted.
    Verified,
    /// The evidence was received but nothing was verified about it.
    Unverified,
    /// The evidence was checked and it does not hold.
    Invalid,
}

impl fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified => f.write_str("VERIFIED"),
            Self::Unverified => f.write_str("UNVERIFIED"),
            Self::Invalid => f.write_str("INVALID"),
        }
    }
}

/// The verification of a piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceVerification {
    /// How far the verification goes.
    pub status: VerificationStatus,
    /// How it was verified, when it was: signature, digest, attestation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Who verified it, when somebody did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_by: Option<String>,
}

impl EvidenceVerification {
    /// Evidence that was received and not verified.
    pub fn unverified() -> Self {
        Self {
            status: VerificationStatus::Unverified,
            method: None,
            verified_by: None,
        }
    }

    /// Evidence that was verified in a given way.
    pub fn verified(method: impl Into<String>) -> Self {
        Self {
            status: VerificationStatus::Verified,
            method: Some(method.into()),
            verified_by: None,
        }
    }

    /// Evidence that was checked and does not hold.
    pub fn invalid(method: impl Into<String>) -> Self {
        Self {
            status: VerificationStatus::Invalid,
            method: Some(method.into()),
            verified_by: None,
        }
    }

    /// Whether the evidence can be weighed as a proof.
    ///
    /// Only verified evidence is a proof. Unverified evidence is a claim,
    /// and invalid evidence is a contradiction: both exist in the record, and
    /// neither counts as a verification.
    pub fn proves(&self) -> bool {
        self.status == VerificationStatus::Verified
    }
}

/// How the evidence was brought to its canonical form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Normalization {
    /// The normalizer that did it.
    pub normalizer: String,
    /// The version of the normalizer. The major version takes part in the
    /// idempotency key: a different major version is a different reading of
    /// the same content.
    pub version: String,
    /// The digest of the canonical result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<crate::domain::content::ContentDigest>,
}

impl Normalization {
    /// A normalization by a given normalizer and version.
    pub fn new(normalizer: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            normalizer: normalizer.into(),
            version: version.into(),
            result_digest: None,
        }
    }

    /// The major version of the normalizer, which is what takes part in the
    /// idempotency key.
    pub fn major_version(&self) -> &str {
        self.version.split('.').next().unwrap_or(&self.version)
    }
}

/// The record of an accepted piece of evidence.
///
/// The record is immutable once accepted: the fields are private and there
/// is no way to change what was accepted. A correction is a new record that
/// supersedes the old one, and the history says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    id: EvidenceId,
    tenant_hrn: Hrn,
    subject_refs: Vec<Hrn>,
    evidence_type: EvidenceType,
    schema_uri: Option<String>,
    schema_version: Option<String>,
    producer: Producer,
    source: SourceRef,
    content: ContentRef,
    observed_at: DateTime<Utc>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    recorded_at: DateTime<Utc>,
    verification: EvidenceVerification,
    normalization: Normalization,
    correlation_id: Option<String>,
}

impl EvidenceRecord {
    /// Starts the building of a record.
    pub fn builder(id: EvidenceId, tenant_hrn: Hrn) -> EvidenceRecordBuilder {
        EvidenceRecordBuilder {
            id,
            tenant_hrn,
            subject_refs: Vec::new(),
            evidence_type: EvidenceType::Other("unspecified".to_string()),
            schema_uri: None,
            schema_version: None,
            producer: Producer::new("unknown", "unknown"),
            source: SourceRef::none(),
            content: ContentRef::by_digest(crate::domain::content::ContentDigest::sha256(
                String::new(),
            )),
            observed_at: Utc::now(),
            valid_from: None,
            valid_to: None,
            recorded_at: Utc::now(),
            verification: EvidenceVerification::unverified(),
            normalization: Normalization::new("none", "0"),
            correlation_id: None,
        }
    }

    /// The identity of the record.
    pub fn id(&self) -> &EvidenceId {
        &self.id
    }

    /// The account the evidence belongs to.
    pub fn tenant_hrn(&self) -> &Hrn {
        &self.tenant_hrn
    }

    /// The subjects the evidence says something about.
    pub fn subject_refs(&self) -> &[Hrn] {
        &self.subject_refs
    }

    /// The kind of evidence.
    pub fn evidence_type(&self) -> &EvidenceType {
        &self.evidence_type
    }

    /// The schema the evidence follows.
    pub fn schema_uri(&self) -> Option<&str> {
        self.schema_uri.as_deref()
    }

    /// The version of that schema.
    pub fn schema_version(&self) -> Option<&str> {
        self.schema_version.as_deref()
    }

    /// Who produced it.
    pub fn producer(&self) -> &Producer {
        &self.producer
    }

    /// Where it came from.
    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    /// The reference to its content.
    pub fn content(&self) -> &ContentRef {
        &self.content
    }

    /// When Hodei learned about it.
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    /// When the source says it became true.
    pub fn valid_from(&self) -> Option<DateTime<Utc>> {
        self.valid_from
    }

    /// When the source says it stopped being true.
    pub fn valid_to(&self) -> Option<DateTime<Utc>> {
        self.valid_to
    }

    /// When it was accepted.
    pub fn recorded_at(&self) -> DateTime<Utc> {
        self.recorded_at
    }

    /// How far its verification goes.
    pub fn verification(&self) -> &EvidenceVerification {
        &self.verification
    }

    /// How it was normalized.
    pub fn normalization(&self) -> &Normalization {
        &self.normalization
    }

    /// The correlation of the request that brought it.
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Checks that the record can be accepted.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        self.content.validate()?;
        if self.tenant_hrn.account_id().is_empty() {
            return Err(EvidenceError::InvalidRecord(
                "an evidence record belongs to an account".to_string(),
            ));
        }
        if let Some(valid_to) = self.valid_to
            && let Some(valid_from) = self.valid_from
            && valid_to < valid_from
        {
            return Err(EvidenceError::InvalidRecord(
                "the evidence stops being true before it starts".to_string(),
            ));
        }
        Ok(())
    }
}

/// The error of a piece of evidence that cannot be accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    /// The content reference is not usable.
    #[error(transparent)]
    Content(#[from] ContentError),
    /// The record is not coherent.
    #[error("the evidence record is not valid: {0}")]
    InvalidRecord(String),
}

/// The builder of an evidence record.
///
/// The builder exists so that the temporal facts can be stated explicitly
/// instead of being taken from the clock when nobody was looking. A record
/// that says when it was observed and when it was recorded says two
/// different things, and the difference matters.
pub struct EvidenceRecordBuilder {
    id: EvidenceId,
    tenant_hrn: Hrn,
    subject_refs: Vec<Hrn>,
    evidence_type: EvidenceType,
    schema_uri: Option<String>,
    schema_version: Option<String>,
    producer: Producer,
    source: SourceRef,
    content: ContentRef,
    observed_at: DateTime<Utc>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    recorded_at: DateTime<Utc>,
    verification: EvidenceVerification,
    normalization: Normalization,
    correlation_id: Option<String>,
}

impl EvidenceRecordBuilder {
    /// The subjects the evidence says something about.
    pub fn subjects(mut self, subjects: Vec<Hrn>) -> Self {
        self.subject_refs = subjects;
        self
    }

    /// The kind of evidence.
    pub fn evidence_type(mut self, evidence_type: EvidenceType) -> Self {
        self.evidence_type = evidence_type;
        self
    }

    /// The schema the evidence follows.
    pub fn schema(mut self, uri: impl Into<String>, version: impl Into<String>) -> Self {
        self.schema_uri = Some(uri.into());
        self.schema_version = Some(version.into());
        self
    }

    /// Who produced it.
    pub fn producer(mut self, producer: Producer) -> Self {
        self.producer = producer;
        self
    }

    /// Where it came from.
    pub fn source(mut self, source: SourceRef) -> Self {
        self.source = source;
        self
    }

    /// The reference to its content.
    pub fn content(mut self, content: ContentRef) -> Self {
        self.content = content;
        self
    }

    /// When Hodei learned about it.
    pub fn observed_at(mut self, moment: DateTime<Utc>) -> Self {
        self.observed_at = moment;
        self
    }

    /// When the source says it became true.
    pub fn valid_from(mut self, moment: DateTime<Utc>) -> Self {
        self.valid_from = Some(moment);
        self
    }

    /// When the source says it stopped being true.
    pub fn valid_to(mut self, moment: DateTime<Utc>) -> Self {
        self.valid_to = Some(moment);
        self
    }

    /// When it was accepted.
    pub fn recorded_at(mut self, moment: DateTime<Utc>) -> Self {
        self.recorded_at = moment;
        self
    }

    /// How far its verification goes.
    pub fn verification(mut self, verification: EvidenceVerification) -> Self {
        self.verification = verification;
        self
    }

    /// How it was normalized.
    pub fn normalization(mut self, normalization: Normalization) -> Self {
        self.normalization = normalization;
        self
    }

    /// The correlation of the request that brought it.
    pub fn correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// The record.
    pub fn build(self) -> EvidenceRecord {
        EvidenceRecord {
            id: self.id,
            tenant_hrn: self.tenant_hrn,
            subject_refs: self.subject_refs,
            evidence_type: self.evidence_type,
            schema_uri: self.schema_uri,
            schema_version: self.schema_version,
            producer: self.producer,
            source: self.source,
            content: self.content,
            observed_at: self.observed_at,
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            recorded_at: self.recorded_at,
            verification: self.verification,
            normalization: self.normalization,
            correlation_id: self.correlation_id,
        }
    }
}
