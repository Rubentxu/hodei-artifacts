//! The trust passport: HSP-126's capsule, extended with the verification
//! record a consumer needs to judge the release **offline**, without any
//! Hodei service.
//!
//! The passport is not a new bundle engine and not a proprietary format.
//! It is the OCI image layout of the capsule — the same blobs, byte for
//! byte — extended with the standard documents the trust chain produces:
//!
//! - the **SLSA v1.2 VSA** (slsa.dev/spec/v1.2/verification_summary): an
//!   in-toto statement whose subject is the capsule image digest and
//!   whose predicate names the verifier, the policy (by digest), the
//!   input attestations and the verification result. The findings in it
//!   are the verdicts of [`assess_provenance`] over a chain that
//!   [`cosign_verify`] actually ran at export time — never declarations
//!   typed into JSON. Hodei's additions (the per-requirement findings,
//!   the chain and assessor versions, the offline limits) ride as
//!   declared custom fields; a consumer that does not know them still
//!   reads a valid VSA.
//! - the **provenance** statement and its DSSE envelope, the envelope
//!   wrapped in a Sigstore-bundle-shaped document (key mode): the
//!   material an offline consumer re-runs the chain against.
//! - the **policy document** the VSA digests, and the **trust roots**
//!   revisions the signature was checked under — public material and
//!   timestamps only, revocations included when there are any.
//! - a **CycloneDX 1.7 SBOM** of the bundle's assets, readable by the
//!   crate's own normalizer ([`read_cyclonedx_17`]).
//!
//! # Hodei's word is evidence, not certification
//!
//! The VSA says what *Hodei* verified, under Hodei's pinned roots, at a
//! frozen moment. A reader that pins the same lineage re-derives the
//! verdicts from the material in the layout; a reader that does not pin
//! it reads a signature by an unknown key — evidence that somebody said
//! something, never a pass. The passport never converts a Hodei decision
//! into a third-party certification: there is no transparency log, no
//! external issuer, and nothing the layout asserts about anyone but
//! itself.
//!
//! # Freshness is never invented offline
//!
//! [`verify_passport_offline`] re-checks what the bytes still prove
//! (digests, PAE signatures, subject binding) and re-runs the chain
//! against the **reader's own roots**: a revocation the reader's roots
//! already carry is refused, because the reader holds that fact. What an
//! offline reading never claims is that no *newer* revocation exists —
//! today's revocation state beyond the reader's own snapshot is
//! [`FreshnessNow::UnknownOffline`], and [`PassportReading::proves_current`]
//! is false until the reader ships an explicit online freshness policy.
//! Unknown is not "not revoked".
//!
//! # Reproducible identity (AC4)
//!
//! The identity of the passport is the digest of its OCI image manifest.
//! The manifest is assembled from deterministic serializations (fixed
//! struct field order, `BTreeMap`-keyed maps) and from the capsule's own
//! bytes, so two exports over the same canonical inputs — same bundle,
//! same capsule, same provenance, same policy, same pinned
//! `verified_at` — reproduce the identical manifest digest and sign
//! identical bytes. Signatures and timestamps are handled explicitly:
//!
//! - the OpenPGP signature lives in a **separate referrer manifest**
//!   (the capsule's pattern), so its non-reproducible packet bytes never
//!   enter the signed manifest's identity;
//! - the passport manifest carries **no creation timestamp of its own**:
//!   the instants that belong to the record (`exported_at` inside the
//!   bundle bytes, `timeVerified` inside the VSA) are export inputs,
//!   already digested in the layers; and
//! - what the identity excludes is declared in the annotations:
//!   `hodei.passport.signatures` names the referrer kept outside the
//!   digest, `hodei.passport.timestamps` names the input-bound instants.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::domain::standards::Sbom;
use hodei_evidence::domain::standards::attestation::{
    AttestationVerification, DsseEnvelope, IN_TOTO_STATEMENT_TYPE, SignatureFailure,
};
use hodei_evidence::standards::{
    ExpectedIdentity, FreshnessNow, RequirementStatus, RootRevision, SLSA_ASSESSMENT_VERSION,
    SlsaAssessment, SlsaBuildLevel, SlsaExpectations, SlsaFinding, SlsaRequirement,
    TRUST_CHAIN_VERSION, TrustRoots, assess_provenance, cosign_verify, read_cyclonedx_17,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::BundleError;
use super::capsule::{
    CAPSULE_ARTIFACT_TYPE, CONFIG_MEDIA_TYPE, Descriptor, INDEX_FILE, INDEX_MEDIA_TYPE,
    ImageManifest, LAYOUT_BLOBS, LAYOUT_FILE, LAYOUT_VERSION, LayoutHeader, LayoutIndex,
    MANIFEST_MEDIA_TYPE, RELEASE_MEDIA_TYPE, SIGNATURE_MEDIA_TYPE, TRUST_MEDIA_TYPE, TrustState,
    annotation, descriptor, digest_of, layout_blob_path, read_declared_blobs, read_json,
    read_layout_blob, read_manifest_of, unsound, write_layout_blob,
};
use super::manifest::{BundleManifest, MANIFEST_NAME};
use crate::openpgp::MetadataSigner;

/// The predicate type of the SLSA v1.2 Verification Summary Attestation.
pub const VSA_PREDICATE_TYPE: &str = "https://slsa.dev/verification_summary/v1";
/// The artifact type of the passport's own image manifest.
pub const PASSPORT_ARTIFACT_TYPE: &str = "application/vnd.hodei.trust-passport.v1";
/// The artifact type of the passport's detached-signature referrer.
pub const PASSPORT_SIGNATURE_ARTIFACT_TYPE: &str = "application/vnd.hodei.passport-signature.v1";

pub const VSA_MEDIA_TYPE: &str = "application/vnd.slsa.verification_summary.v1+json";
pub const PROVENANCE_STATEMENT_MEDIA_TYPE: &str = "application/vnd.in-toto.statement.v1+json";
pub const SIGSTORE_BUNDLE_MEDIA_TYPE: &str = "application/vnd.hodei.sigstore-bundle.v1+json";
pub const POLICY_MEDIA_TYPE: &str = "application/vnd.hodei.verification-policy.v1+json";
pub const SBOM_MEDIA_TYPE: &str = "application/vnd.cyclonedx+json";
pub const TRUST_ROOTS_MEDIA_TYPE: &str = "application/vnd.hodei.trust-roots.v1+json";

/// The version of this passport writer, named in the VSA verifier id.
pub const PASSPORT_VERSION: &str = "1.0.0";

/// The refusal codes of a passport reading. Stable identifiers a
/// consumer can branch on without string matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PassportProblemCode {
    /// A blob or a layer the layout declares is missing: the evidence
    /// is not all there, and "no revocation found" is not an option.
    MissingEvidence,
    /// A blob digests differently than declared: the artifact was
    /// manipulated.
    TamperedBlob,
    /// The VSA names a policy whose bytes differ from the policy layer,
    /// or the reader judges under a different policy than the record
    /// was written under.
    PolicyMismatch,
    /// The VSA's verdicts do not match what the material in the layout
    /// verifies to, now, under the reader's own roots. This is the
    /// anti-auto-affirmation gate.
    VerdictMismatch,
    /// The provenance's signing key is revoked under the reader's roots
    /// at the reading moment: the bytes were said, the trust is gone.
    SignerRevoked,
    /// The VSA does not speak about the capsule the passport names as
    /// its subject.
    SubjectMismatch,
    /// A document in the passport cannot be read.
    Malformed,
    /// The OpenPGP signature over the passport manifest does not hold,
    /// or names a different key than the reader verifies with.
    SignatureInvalid,
}

/// The refusal of a passport reading, with its code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the passport is not sound [{code:?}]: {detail}")]
pub struct PassportProblem {
    pub code: PassportProblemCode,
    pub detail: String,
}

impl PassportProblem {
    fn new(code: PassportProblemCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl From<PassportProblem> for BundleError {
    fn from(problem: PassportProblem) -> Self {
        BundleError::Unsound(problem.to_string())
    }
}

/// One piece of the input record: a DSSE envelope plus the public point
/// its signature verifies under, as bytes and key id. The point (never
/// a secret) is what lets the passport — and any external tool —
/// re-check the envelope offline without asking a service. The
/// statement travels decoded from the envelope's payload: the passport
/// refuses an envelope whose payload is not readable, at export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedDocument {
    /// The DSSE envelope exactly as it was signed.
    pub envelope_json: String,
    /// The key id the envelope's signature names.
    pub key_id: String,
    /// The uncompressed ECDSA P-256 public point (65 bytes, `0x04`).
    pub public_point: Vec<u8>,
}

/// The policy document the VSA's `policy` field digests, and that the
/// reader must bring to the verification to judge the record under the
/// same expectations it was written under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportPolicy {
    /// An addressable name of the policy.
    pub uri: String,
    /// The version of the policy.
    pub version: String,
    /// The builder identities the policy trusts.
    pub trusted_builders: Vec<String>,
    /// The issuer the signing root must be pinned to, when pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_issuer: Option<String>,
    /// What the deployment established beyond the statement: the
    /// builder's own identity signed the provenance.
    #[serde(default)]
    pub builder_identity_proven: bool,
    /// What the deployment established: the declared inputs are the
    /// whole input set (the hermetic completeness JSON cannot say).
    #[serde(default)]
    pub hermetic_declared_complete: bool,
}

/// Everything the export needs. `verified_at` is an input, not wall
/// clock: AC4's reproducibility is "same canonical inputs, same
/// manifest identity".
#[derive(Debug, Clone)]
pub struct PassportInputs<'a> {
    /// The bundle directory written by HSP-126's export. Its manifest
    /// bytes are the passport's release layer byte for byte.
    pub bundle_dir: &'a Path,
    /// The capsule layout written by HSP-126 beside it. The passport
    /// extends this capsule; it never re-derives its blobs from scratch.
    pub capsule_dir: &'a Path,
    /// The image digest of the capsule: the subject of the VSA and of
    /// the passport manifest's `subject`.
    pub capsule_image_digest: String,
    /// The provenance statement, DSSE-signed, whose subject must name
    /// the capsule image digest.
    pub provenance: SignedDocument,
    /// The policy the evaluation reads; it becomes a layer.
    pub policy: PassportPolicy,
    /// The roots the chain is verified against at export time.
    pub roots: &'a TrustRoots,
    /// The revisions to publish as the trust-roots document (public
    /// material only). The export refuses if the signing key — when it
    /// is one of the roots — is not among them: the passport documents
    /// exactly the roots its own verdicts were reached under.
    pub published_revisions: Vec<RootRevision>,
    /// The instant the verification is frozen at.
    pub verified_at: DateTime<Utc>,
}

/// What an export produced.
#[derive(Debug, Clone)]
pub struct PassportExport {
    /// The layout directory written.
    pub layout_dir: PathBuf,
    /// The digest of the passport image manifest — the identity AC4
    /// reproves across two exports.
    pub image_digest: String,
    /// The exact bytes that were signed.
    pub signed_manifest: Vec<u8>,
    /// The digest of the VSA layer.
    pub vsa_digest: String,
    /// The digest of the signature referrer manifest (outside the
    /// identity above).
    pub signature_digest: String,
    /// The level the ladder asserted at export. Kept in the report so
    /// the exporter cannot claim a level it did not get: the VSA is
    /// written from exactly this.
    pub asserted_level: Option<SlsaBuildLevel>,
    /// Whether the provenance signature held under the export's roots.
    pub cryptographic_holds: bool,
}

/// The in-toto subject of the VSA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportSubject {
    pub name: String,
    /// Digests by algorithm, lowercase hex without prefix (the in-toto
    /// convention).
    pub digest: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VsaVerifier {
    pub id: String,
    pub version: BTreeMap<String, String>,
}

/// A resource the verification consumed, by digest (the SLSA
/// ResourceDescriptor fields this passport needs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VsaResource {
    pub uri: String,
    pub digest: BTreeMap<String, String>,
}

/// One requirement verdict as the VSA records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsaFinding {
    /// The requirement name, snake_case (the ladder's own vocabulary).
    pub requirement: String,
    /// `met` / `not_met` / `not_evaluated`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
}

/// The VSA predicate: the SLSA v1.2 fields, in the spec's camelCase,
/// plus Hodei's declared additions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VsaPredicate {
    pub verifier: VsaVerifier,
    /// The frozen moment of the verification (RFC 3339, UTC).
    pub time_verified: String,
    /// Where the subject lives when served.
    pub resource_uri: String,
    pub policy: VsaResource,
    pub input_attestations: Vec<VsaResource>,
    /// `PASSED`, `FAILED`, or `SLSA_BUILD_LEVEL_n`.
    pub verification_result: String,
    pub verified_levels: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependency_levels: BTreeMap<String, u32>,
    pub slsa_version: String,
    /// The per-requirement verdicts of the ladder run at export time.
    /// A declared addition to the predicate, never a substitution of
    /// the SLSA fields above.
    pub hodei_findings: Vec<VsaFinding>,
    /// The versions of the HSP-154 components that produced the record.
    pub hodei_assessor_version: String,
    pub hodei_chain_version: String,
    /// The lineage and revision the signature was checked against, when
    /// the key was one of the export's roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hodei_root_lineage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hodei_root_revision: Option<u32>,
    /// The honest offline boundary, in the record itself: what this VSA
    /// does *not* claim about today.
    pub hodei_offline_limits: VsaOfflineLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VsaOfflineLimits {
    /// No transparency-log inclusion was checked.
    pub no_transparency_log: bool,
    /// Today's revocation state of the signing root is unknown offline
    /// unless the reader ships fresher roots than the snapshot.
    pub revocation_freshness: String,
    /// The VSA records Hodei's verification; it is not a third-party
    /// certification.
    pub issuer_kind: String,
}

/// The VSA statement itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportVsa {
    #[serde(rename = "_type")]
    pub statement_type: String,
    pub subject: Vec<PassportSubject>,
    #[serde(rename = "predicateType")]
    pub predicate_type: String,
    pub predicate: VsaPredicate,
}

/// One trust-root revision as the passport lays it down: public point
/// hex and timestamps, never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportRootRevision {
    pub lineage: String,
    pub revision: u32,
    pub key_id: String,
    /// Lowercase hex of the uncompressed P-256 point.
    pub public_point_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub activated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

/// The trust-roots document: the revisions the record's verdicts were
/// reached under, sorted by lineage and revision. Public material only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportTrustRoots {
    pub revisions: Vec<PassportRootRevision>,
}

fn revision_of(revision: &RootRevision) -> PassportRootRevision {
    PassportRootRevision {
        lineage: revision.lineage.clone(),
        revision: revision.revision,
        key_id: revision.key.key_id.clone(),
        public_point_hex: point_hex(&revision.key.point),
        issuer: revision.issuer.clone(),
        activated_at: revision.activated_at.to_rfc3339(),
        retired_at: revision.retired_at.map(|at| at.to_rfc3339()),
        revoked_at: revision.revoked_at.map(|at| at.to_rfc3339()),
    }
}

fn point_hex(point: &[u8]) -> String {
    point.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The hex of a `sha256:<hex>` digest string, without the prefix.
fn hex_of(digest: &str) -> &str {
    digest.split_once(':').map(|(_, hex)| hex).unwrap_or(digest)
}

/// The canonical bytes of the envelope every digest in the passport
/// uses: the JSON parsed to a value and written back compactly. Both
/// sides run exactly this round-trip — writer before publishing, reader
/// after unpacking the bundle — so the VSA's `inputAttestations` digest
/// is checkable offline without depending on the whitespace the
/// envelope happened to arrive in, and the round-trip is idempotent.
fn canonical_envelope_bytes(envelope_json: &str) -> Result<Vec<u8>, PassportProblem> {
    // Parse once through the crate's envelope type: an input that is
    // not a DSSE envelope is refused here, at export, not silently.
    serde_json::from_str::<DsseEnvelope>(envelope_json).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the provenance envelope is not a DSSE envelope: {error}"),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(envelope_json).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the provenance envelope is not JSON: {error}"),
        )
    })?;
    serde_json::to_vec(&value).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the envelope cannot be canonicalized: {error}"),
        )
    })
}

/// Pull the canonical envelope bytes back out of the bundle layer: the
/// inverse of [`sigstore_bundle_json`]. The same round-trip as
/// [`canonical_envelope_bytes`] applied to the stored object, hence
/// byte-identical.
fn envelope_from_bundle(bundle_bytes: &[u8]) -> Result<Vec<u8>, PassportProblem> {
    let bundle: serde_json::Value = serde_json::from_slice(bundle_bytes).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the sigstore bundle cannot be read: {error}"),
        )
    })?;
    let envelope = bundle.get("dsseEnvelope").ok_or_else(|| {
        PassportProblem::new(
            PassportProblemCode::MissingEvidence,
            "the bundle carries no DSSE envelope",
        )
    })?;
    serde_json::to_vec(envelope).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the envelope cannot be written back: {error}"),
        )
    })
}

/// The level of the VSA's result string as a number.
fn level_number(level: SlsaBuildLevel) -> u8 {
    match level {
        SlsaBuildLevel::L1 => 1,
        SlsaBuildLevel::L2 => 2,
        SlsaBuildLevel::L3 => 3,
    }
}

/// The VSA's `verifiedLevels`: every level up to and including the
/// asserted one, because SLSA levels accumulate.
fn verified_levels(level: Option<SlsaBuildLevel>) -> Vec<String> {
    (1..=match level {
        Some(level) => level_number(level),
        None => 0,
    })
        .map(|number| format!("SLSA_BUILD_LEVEL_{number}"))
        .collect()
}

/// The VSA's result from the real assessment: a level when the ladder
/// asserts one, FAILED when a floor requirement refused.
fn verification_result(assessment: &SlsaAssessment) -> String {
    if !assessment.unmet_at(SlsaBuildLevel::L1).is_empty() {
        return "FAILED".to_string();
    }
    match assessment.asserted_level() {
        Some(level) => format!("SLSA_BUILD_LEVEL_{}", level_number(level)),
        None => "SLSA_BUILD_LEVEL_0".to_string(),
    }
}

/// The findings in the VSA's vocabulary: the same names and statuses
/// the ladder serializes, so a reader compares structures, not text.
fn findings_of(assessment: &SlsaAssessment) -> Vec<VsaFinding> {
    assessment
        .findings
        .iter()
        .map(|finding| {
            let (status, reason, gap) = match &finding.status {
                RequirementStatus::Met => ("met", None, None),
                RequirementStatus::NotMet { reason } => ("not_met", Some(reason.clone()), None),
                RequirementStatus::NotEvaluated { gap } => {
                    ("not_evaluated", None, Some(gap.clone()))
                }
            };
            VsaFinding {
                requirement: requirement_name(finding.requirement).to_string(),
                status: status.to_string(),
                reason,
                gap,
            }
        })
        .collect()
}

fn requirement_name(requirement: SlsaRequirement) -> &'static str {
    match requirement {
        SlsaRequirement::ProvenanceAvailable => "provenance_available",
        SlsaRequirement::ProvenanceSigned => "provenance_signed",
        SlsaRequirement::SubjectVerified => "subject_verified",
        SlsaRequirement::BuilderTrusted => "builder_trusted",
        SlsaRequirement::IssuerPinned => "issuer_pinned",
        SlsaRequirement::NonFalsifiable => "non_falsifiable",
        SlsaRequirement::HermeticBuild => "hermetic_build",
    }
}

/// Turns the VSA's findings back into the ladder's own model so the
/// offline comparison is structural, never textual.
fn assessment_from_findings(
    assessor_version: &str,
    findings: &[VsaFinding],
) -> Result<SlsaAssessment, PassportProblem> {
    let mut out = Vec::with_capacity(findings.len());
    for finding in findings {
        let requirement = match finding.requirement.as_str() {
            "provenance_available" => SlsaRequirement::ProvenanceAvailable,
            "provenance_signed" => SlsaRequirement::ProvenanceSigned,
            "subject_verified" => SlsaRequirement::SubjectVerified,
            "builder_trusted" => SlsaRequirement::BuilderTrusted,
            "issuer_pinned" => SlsaRequirement::IssuerPinned,
            "non_falsifiable" => SlsaRequirement::NonFalsifiable,
            "hermetic_build" => SlsaRequirement::HermeticBuild,
            other => {
                return Err(PassportProblem::new(
                    PassportProblemCode::Malformed,
                    format!("the VSA names a requirement this build does not know: {other}"),
                ));
            }
        };
        let status = match finding.status.as_str() {
            "met" => RequirementStatus::Met,
            "not_met" => RequirementStatus::NotMet {
                reason: finding.reason.clone().unwrap_or_default(),
            },
            "not_evaluated" => RequirementStatus::NotEvaluated {
                gap: finding.gap.clone().unwrap_or_default(),
            },
            other => {
                return Err(PassportProblem::new(
                    PassportProblemCode::Malformed,
                    format!("the VSA names a status this build does not know: {other}"),
                ));
            }
        };
        out.push(SlsaFinding {
            requirement,
            status,
        });
    }
    Ok(SlsaAssessment {
        assessor_version: assessor_version.to_string(),
        findings: out,
    })
}

/// The statement payload of an envelope, decoded from its base64:
/// the provenance layer's bytes come from the envelope itself, never
/// from a caller's side-channel copy.
fn statement_from_envelope(envelope_json: &str) -> Result<Vec<u8>, PassportProblem> {
    let envelope: DsseEnvelope = serde_json::from_str(envelope_json).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the provenance envelope is not a DSSE envelope: {error}"),
        )
    })?;
    BASE64.decode(&envelope.payload).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the envelope payload is not base64: {error}"),
        )
    })
}

/// The Sigstore-bundle-shaped document around the canonical envelope
/// (v0.3 mediaType, key mode): the envelope plus the public point its
/// signature verifies under, so the consumer needs no service to
/// re-check it. The `hint` follows the sigstore convention of a
/// base64 digest of the key; the raw point travels beside it as hex
/// because the point is exactly what `VerificationKey` carries.
fn sigstore_bundle_json(
    provenance: &SignedDocument,
    envelope_bytes: &[u8],
) -> Result<String, PassportProblem> {
    use sha2::Digest as _;
    let envelope: serde_json::Value = serde_json::from_slice(envelope_bytes).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the canonical envelope is not JSON: {error}"),
        )
    })?;
    let hint = BASE64.encode(sha2::Sha256::digest(&provenance.public_point));
    let bundle = json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {
            "publicKey": {
                "hint": hint,
                "keyId": provenance.key_id,
                "ecdsaP256UncompressedPointHex": point_hex(&provenance.public_point),
            }
        },
        "dsseEnvelope": envelope,
    });
    serde_json::to_string_pretty(&bundle).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the bundle cannot be written: {error}"),
        )
    })
}

/// The CycloneDX 1.7 SBOM of the bundle: the capsule image as the
/// subject component, every exported asset as a component with its
/// sha256. The crate's own normalizer reads it back — the SBOM is a
/// claim the consumer can parse, not decoration.
fn sbom_json(manifest: &BundleManifest, capsule_image_digest: &str) -> String {
    let components: Vec<serde_json::Value> = manifest
        .assets
        .iter()
        .map(|asset| {
            json!({
                "bom-ref": format!("pkg:hodei/{}@{}", asset.package, asset.version),
                "type": "file",
                "name": asset.asset_path,
                "version": asset.version,
                "purl": format!("pkg:generic/{}@{}", asset.package, asset.version),
                "hashes": [{ "alg": "SHA-256", "content": hex_of(&asset.digest) }],
            })
        })
        .collect();
    let bom = json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.7",
        "version": 1,
        "metadata": {
            "component": {
                "bom-ref": "pkg:oci/hodei-capsule",
                "type": "container",
                "name": format!(
                    "{}/{}:capsule",
                    manifest.origin.account, manifest.origin.repository
                ),
                "version": hex_of(capsule_image_digest),
                "hashes": [{ "alg": "SHA-256", "content": hex_of(capsule_image_digest) }],
            }
        },
        "components": components,
    });
    serde_json::to_vec_pretty(&bom)
        .map(|bytes| String::from_utf8(bytes).expect("JSON is UTF-8"))
        .expect("the SBOM serializes")
}

/// What a **reader** brings to the offline check: its own pinned roots,
/// its own copy of the policy document, its own pins. The reader never
/// trusts what the passport says about its own verification — it
/// re-runs it.
#[derive(Debug, Clone)]
pub struct ReaderExpectations<'a> {
    pub roots: &'a TrustRoots,
    /// The policy the reader judges under. It must be byte-identical
    /// to the policy layer the VSA digests, or the comparison would be
    /// meaningless: that is `PolicyMismatch`, not a silent
    /// reinterpretation.
    pub policy: &'a PassportPolicy,
    /// The issuer the reader pins. When `None` the reader re-checks the
    /// record without pinning identity — the crypto can still hold, and
    /// the ladder says `NotEvaluated` about the pin.
    pub issuer: Option<String>,
    /// When set, the reading evaluates freshness **online** at the
    /// reading moment with this threshold; when `None`, the reading is
    /// offline and freshness is `UnknownOffline`, full stop.
    pub online_freshness: Option<Duration>,
}

/// What a reading of a passport produced.
#[derive(Debug, Clone)]
pub struct PassportReading {
    /// The capsule image digest the passport is about.
    pub capsule_image_digest: String,
    /// The digest of the passport image manifest.
    pub passport_image_digest: String,
    /// The bundle manifest the passport carries, parsed from the exact
    /// bytes of the release layer: origin and temporal validity come
    /// out of the record itself, never from a service.
    pub manifest: BundleManifest,
    /// The trust state of the bundle at export, re-derived from those
    /// bytes (the capsule's own rule, repeated here).
    pub trust_state: TrustState,
    /// The VSA as the layout holds it.
    pub vsa: PassportVsa,
    /// The verdicts the VSA recorded.
    pub recorded: SlsaAssessment,
    /// The verdicts the reader just re-derived from the material under
    /// its own roots. The reading succeeded only because they match,
    /// requirement by requirement.
    pub recomputed: SlsaAssessment,
    /// Whether the provenance signature holds under the reader's roots.
    pub cryptographic_holds: bool,
    /// The lineage/revision the signature was checked against, when the
    /// reader's roots know the key.
    pub root: Option<(String, u32)>,
    /// The freshness of the record's revocation state **today**.
    /// Offline it is `UnknownOffline`: the reader may know the crypto
    /// still holds, and may not claim today's revocations are fresh or
    /// nonexistent.
    pub freshness: FreshnessNow,
    /// The OpenPGP key that signed the passport manifest.
    pub signer_fingerprint: String,
    /// The moment the record was frozen at (from the VSA itself).
    pub verified_at: DateTime<Utc>,
}

impl PassportReading {
    /// Whether the reading is a current proof: the crypto holds and
    /// freshness is *known* fresh. An offline reading is never one:
    /// `UnknownOffline` is not a green light.
    pub fn proves_current(&self) -> bool {
        self.cryptographic_holds && self.freshness == FreshnessNow::Fresh
    }
}

/// Write the passport layout at `layout` on top of the capsule HSP-126
/// already exported.
///
/// The chain verification and the SLSA ladder run **here, now, over the
/// provenance bytes** — the VSA is written from their output, never
/// from a declaration (AC1). A provenance whose signature does not hold
/// is exported as a FAILED VSA rather than refused: a release that
/// could not be verified deserves a record that says so honestly. What
/// this refuses to write is a record that would lie: a signing key from
/// outside the published roots.
pub fn export_passport(
    inputs: &PassportInputs<'_>,
    layout: &Path,
    signer: &MetadataSigner,
) -> Result<PassportExport, BundleError> {
    if layout.exists() {
        return Err(BundleError::Refused(format!(
            "{} already exists: the export never overwrites a passport",
            layout.display()
        )));
    }

    // The bundle is the base of everything: the passport carries its
    // manifest bytes and refuses a bundle this binary would not import.
    let manifest: BundleManifest = read_json(&inputs.bundle_dir.join(MANIFEST_NAME))?;
    manifest.check_importable()?;
    let bundle_bytes = std::fs::read(inputs.bundle_dir.join(MANIFEST_NAME))?;

    // The capsule's image manifest, read through the capsule's own
    // rules: the passport extends what verify_capsule accepts.
    let capsule_index: LayoutIndex = read_json(&inputs.capsule_dir.join(INDEX_FILE))?;
    let capsule_image =
        read_manifest_of(inputs.capsule_dir, &capsule_index, CAPSULE_ARTIFACT_TYPE)?;
    let capsule_desc = capsule_index
        .manifests
        .iter()
        .find(|desc| desc.digest == inputs.capsule_image_digest)
        .ok_or_else(|| unsound("the capsule index does not announce the image digest"))?;
    let capsule_image_bytes = read_layout_blob(inputs.capsule_dir, capsule_desc)?;
    let capsule_image_digest = inputs.capsule_image_digest.clone();
    if serde_json::from_slice::<ImageManifest>(&capsule_image_bytes)? != capsule_image {
        return Err(unsound(
            "the capsule image manifest is not the one the index names",
        ));
    }

    std::fs::create_dir_all(layout.join(LAYOUT_BLOBS))?;

    // 1. The chain the HSP-154 way: envelope, pinned roots, subject
    //    binding, identity pins. The subject the chain must vouch for
    //    is the capsule image digest. The envelope enters canonically,
    //    and everything the record digests uses those same bytes.
    let envelope_bytes =
        canonical_envelope_bytes(&inputs.provenance.envelope_json).map_err(BundleError::from)?;
    let envelope_json = String::from_utf8(envelope_bytes.clone())
        .map_err(|_| unsound("the canonical envelope is not UTF-8"))?;
    let statement_bytes = statement_from_envelope(&envelope_json).map_err(BundleError::from)?;
    let expected = ExpectedIdentity {
        subject_algorithm: "sha256".to_string(),
        subject_digest: hex_of(&capsule_image_digest).to_string(),
        issuer: inputs.policy.pinned_issuer.clone(),
        builder_id: inputs.policy.trusted_builders.first().cloned(),
    };
    let chain = cosign_verify(&envelope_json, inputs.roots, &expected, inputs.verified_at)
        .map_err(|error| BundleError::Read(format!("the provenance cannot be read: {error}")))?;

    // The signing key, when the chain found it among the roots, must be
    // one of the revisions this passport publishes: the record
    // documents exactly the roots its verdicts were reached under, and
    // a foreign key is refused rather than laundered into JSON.
    let signing_key_id = match &chain.attestation.verification {
        AttestationVerification::Verified { key_id }
        | AttestationVerification::Invalid {
            reason: SignatureFailure::Revoked { key_id },
        }
        | AttestationVerification::Invalid {
            reason: SignatureFailure::UnknownKey(key_id),
        } => Some(key_id.clone()),
        _ => None,
    };
    if let Some(key_id) = &signing_key_id
        && inputs.roots.find_key(key_id).is_some()
        && !inputs
            .published_revisions
            .iter()
            .any(|revision| revision.key.key_id == *key_id)
    {
        return Err(BundleError::Unsound(format!(
            "the provenance names key {key_id}, which is not among the revisions the passport would publish"
        )));
    }

    let assessment = assess_provenance(
        &chain,
        &expected,
        &SlsaExpectations {
            trusted_builder_ids: inputs.policy.trusted_builders.clone(),
            pinned_issuer: inputs.policy.pinned_issuer.clone(),
            builder_identity_proven: inputs.policy.builder_identity_proven,
            hermetic_declared_complete: inputs.policy.hermetic_declared_complete,
        },
    );

    // 2. The capsule's blobs first, byte for byte through the same
    //    writer: content addressing is identical everywhere.
    let capsule_blobs = read_declared_blobs(inputs.capsule_dir, &capsule_image)?;
    for desc in std::iter::once(&capsule_image.config).chain(capsule_image.layers.iter()) {
        let bytes = capsule_blobs
            .get(&desc.digest)
            .ok_or_else(|| unsound(format!("the capsule lost its own blob {}", desc.digest)))?;
        let actual = write_layout_blob(layout, &desc.media_type, bytes)?;
        if actual.digest != desc.digest {
            return Err(unsound(format!(
                "the capsule blob {} cannot be reproduced byte for byte",
                desc.digest
            )));
        }
    }

    // 3. The passport's own documents, appended as named layers. The
    //    capsule's layers keep their place and their digests.
    let mut layers = capsule_image.layers.clone();
    let mut push = |media_type: &str, title: &str, bytes: &[u8]| -> Result<(), BundleError> {
        let mut desc = write_layout_blob(layout, media_type, bytes)?;
        desc.annotations.insert(
            "org.opencontainers.image.title".to_string(),
            title.to_string(),
        );
        layers.push(desc);
        Ok(())
    };
    push(
        PROVENANCE_STATEMENT_MEDIA_TYPE,
        "provenance.json",
        &statement_bytes,
    )?;
    let statement_digest = digest_of(&statement_bytes);
    let bundle_doc =
        sigstore_bundle_json(&inputs.provenance, &envelope_bytes).map_err(BundleError::from)?;
    push(
        SIGSTORE_BUNDLE_MEDIA_TYPE,
        "provenance.bundle.json",
        bundle_doc.as_bytes(),
    )?;
    let policy_doc = serde_json::to_vec_pretty(&inputs.policy)?;
    push(POLICY_MEDIA_TYPE, "policy.json", &policy_doc)?;
    let mut revisions: Vec<PassportRootRevision> =
        inputs.published_revisions.iter().map(revision_of).collect();
    revisions.sort_by(|a, b| (&a.lineage, a.revision).cmp(&(&b.lineage, b.revision)));
    let roots_doc = serde_json::to_vec_pretty(&PassportTrustRoots { revisions })?;
    push(TRUST_ROOTS_MEDIA_TYPE, "trust-roots.json", &roots_doc)?;
    push(
        SBOM_MEDIA_TYPE,
        "sbom.cdx.json",
        sbom_json(&manifest, &capsule_image_digest).as_bytes(),
    )?;

    // 4. The VSA, written from the assessment — including its refusal.
    let vsa = PassportVsa {
        statement_type: IN_TOTO_STATEMENT_TYPE.to_string(),
        subject: vec![PassportSubject {
            name: format!(
                "{}/{}:capsule",
                manifest.origin.account, manifest.origin.repository
            ),
            digest: BTreeMap::from([(
                "sha256".to_string(),
                hex_of(&capsule_image_digest).to_string(),
            )]),
        }],
        predicate_type: VSA_PREDICATE_TYPE.to_string(),
        predicate: VsaPredicate {
            verifier: VsaVerifier {
                id: "https://hodei.dev/verifier/hodei-artifacts".to_string(),
                version: BTreeMap::from([(
                    "hodei-artifacts".to_string(),
                    format!("trust-passport/{PASSPORT_VERSION}"),
                )]),
            },
            time_verified: inputs.verified_at.to_rfc3339(),
            resource_uri: format!(
                "oci://{}/{}@{}",
                manifest.origin.account, manifest.origin.repository, capsule_image_digest
            ),
            policy: VsaResource {
                uri: format!("{}#{}", inputs.policy.uri, inputs.policy.version),
                digest: BTreeMap::from([(
                    "sha256".to_string(),
                    hex_of(&digest_of(&policy_doc)).to_string(),
                )]),
            },
            input_attestations: vec![
                VsaResource {
                    uri: "provenance.bundle.json".to_string(),
                    digest: BTreeMap::from([(
                        "sha256".to_string(),
                        hex_of(&digest_of(&envelope_bytes)).to_string(),
                    )]),
                },
                VsaResource {
                    uri: "provenance.json".to_string(),
                    digest: BTreeMap::from([(
                        "sha256".to_string(),
                        hex_of(&statement_digest).to_string(),
                    )]),
                },
            ],
            verification_result: verification_result(&assessment),
            verified_levels: verified_levels(assessment.asserted_level()),
            dependency_levels: BTreeMap::new(),
            slsa_version: "1.2".to_string(),
            hodei_findings: findings_of(&assessment),
            hodei_assessor_version: SLSA_ASSESSMENT_VERSION.to_string(),
            hodei_chain_version: TRUST_CHAIN_VERSION.to_string(),
            hodei_root_lineage: chain.root_lineage.clone(),
            hodei_root_revision: chain.root_revision,
            hodei_offline_limits: VsaOfflineLimits {
                no_transparency_log: true,
                revocation_freshness: "unknown-offline-today".to_string(),
                issuer_kind: "hodei-own-verification".to_string(),
            },
        },
    };
    let vsa_bytes = serde_json::to_vec_pretty(&vsa)?;
    push(VSA_MEDIA_TYPE, "vsa.json", &vsa_bytes)?;
    let vsa_digest = digest_of(&vsa_bytes);

    // 5. The passport image manifest. Its annotations derive from the
    //    documents and the capsule only — no wall clock of its own. The
    //    instants that belong to the record already live inside the
    //    layer bytes (the bundle's exported_at, the VSA's timeVerified),
    //    which this manifest digests.
    let mut map = annotation(&[(
        "hodei.passport",
        &format!("hodei.trust-passport/{PASSPORT_VERSION}"),
    )]);
    map.insert(
        "hodei.passport.subject".to_string(),
        capsule_image_digest.clone(),
    );
    map.insert(
        "hodei.passport.bundle".to_string(),
        digest_of(&bundle_bytes),
    );
    map.insert("hodei.passport.vsa".to_string(), vsa_digest.clone());
    map.insert(
        "hodei.passport.assessment".to_string(),
        verification_result(&assessment),
    );
    // The explicit treatment AC4 asks for, declared in the manifest:
    // what is outside this identity and why.
    map.insert(
        "hodei.passport.signatures".to_string(),
        "detached-openpgp-referrer-excluded-from-identity".to_string(),
    );
    map.insert(
        "hodei.passport.timestamps".to_string(),
        "input-declared-in-layers; no clock in manifest".to_string(),
    );
    let image = ImageManifest {
        schema_version: 2,
        media_type: MANIFEST_MEDIA_TYPE.to_string(),
        artifact_type: PASSPORT_ARTIFACT_TYPE.to_string(),
        config: descriptor(CONFIG_MEDIA_TYPE, b"{}"),
        layers,
        subject: Some(Descriptor {
            media_type: MANIFEST_MEDIA_TYPE.to_string(),
            digest: capsule_image_digest.clone(),
            size: capsule_image_bytes.len() as u64,
            annotations: BTreeMap::new(),
        }),
        annotations: map,
    };
    let image_bytes = serde_json::to_vec_pretty(&image)?;
    let image_digest = digest_of(&image_bytes);
    std::fs::write(layout.join(layout_blob_path(&image_digest)), &image_bytes)?;
    // The subject's bytes travel with the passport: the layout is
    // self-contained, and a registry client resolves the subject
    // without the capsule sitting beside it.
    write_layout_blob(layout, MANIFEST_MEDIA_TYPE, &capsule_image_bytes)?;

    // 6. The detached OpenPGP signature over the exact manifest bytes,
    //    as a referrer manifest — the capsule's pattern, so the
    //    signature's non-reproducible packet bytes never enter the
    //    signed manifest's identity.
    let signature = signer
        .sign_detached(&image_bytes)
        .map_err(|error| unsound(format!("the passport cannot be signed: {error}")))?;
    let signature_json = serde_json::to_vec_pretty(&json!({
        "format": "openpgp-detached",
        "signer": signer.fingerprint_hex(),
        "signature": String::from_utf8_lossy(&signature),
    }))?;
    let signature_layer = write_layout_blob(layout, SIGNATURE_MEDIA_TYPE, &signature_json)?;
    let signature_image = ImageManifest {
        schema_version: 2,
        media_type: MANIFEST_MEDIA_TYPE.to_string(),
        artifact_type: PASSPORT_SIGNATURE_ARTIFACT_TYPE.to_string(),
        config: descriptor(CONFIG_MEDIA_TYPE, b"{}"),
        layers: vec![signature_layer],
        subject: Some(Descriptor {
            media_type: MANIFEST_MEDIA_TYPE.to_string(),
            digest: image_digest.clone(),
            size: image_bytes.len() as u64,
            annotations: BTreeMap::new(),
        }),
        annotations: BTreeMap::new(),
    };
    let signature_bytes = serde_json::to_vec_pretty(&signature_image)?;
    let signature_digest = digest_of(&signature_bytes);
    std::fs::write(
        layout.join(layout_blob_path(&signature_digest)),
        &signature_bytes,
    )?;

    let index = LayoutIndex {
        schema_version: 2,
        media_type: INDEX_MEDIA_TYPE.to_string(),
        manifests: vec![
            Descriptor {
                media_type: MANIFEST_MEDIA_TYPE.to_string(),
                digest: image_digest.clone(),
                size: image_bytes.len() as u64,
                annotations: annotation(&[("org.opencontainers.image.ref.name", "latest")]),
            },
            Descriptor {
                media_type: MANIFEST_MEDIA_TYPE.to_string(),
                digest: signature_digest.clone(),
                size: signature_bytes.len() as u64,
                annotations: annotation(&[("org.opencontainers.image.ref.name", "latest.sig")]),
            },
        ],
    };
    std::fs::write(layout.join(INDEX_FILE), serde_json::to_vec_pretty(&index)?)?;
    std::fs::write(
        layout.join(LAYOUT_FILE),
        serde_json::to_vec_pretty(&LayoutHeader {
            image_layout_version: LAYOUT_VERSION.to_string(),
        })?,
    )?;

    Ok(PassportExport {
        layout_dir: layout.to_path_buf(),
        image_digest,
        signed_manifest: image_bytes,
        vsa_digest,
        signature_digest,
        asserted_level: assessment.asserted_level(),
        cryptographic_holds: chain.cryptographic_holds,
    })
}

/// Import and read a passport **in isolation**: no service, no network,
/// the reader's own pinned roots. Every declared blob is verified; the
/// VSA is compared requirement by requirement against a fresh
/// evaluation of the material in the layout; the record's own digests
/// (policy, envelope) are re-checked against the bytes; freshness is
/// reported honestly.
///
/// What it proves: the bytes still verify the way the record says they
/// verified then, under roots the reader itself trusts.
/// What it never says: "no revocation is known, therefore valid today."
pub fn verify_passport_offline(
    layout: &Path,
    signer: &MetadataSigner,
    reader: &ReaderExpectations<'_>,
    now: DateTime<Utc>,
) -> Result<PassportReading, BundleError> {
    let header: LayoutHeader = read_json(&layout.join(LAYOUT_FILE))?;
    if header.image_layout_version != LAYOUT_VERSION {
        return Err(unsound(format!(
            "unsupported image layout version {}",
            header.image_layout_version
        )));
    }
    let index: LayoutIndex = read_json(&layout.join(INDEX_FILE))?;
    if index.media_type != INDEX_MEDIA_TYPE {
        return Err(unsound(format!("{} is not an OCI index", index.media_type)));
    }

    let image = read_manifest_of(layout, &index, PASSPORT_ARTIFACT_TYPE)?;
    // Every declared blob checked against its descriptor: a missing
    // blob is missing-evidence, a swapped one is tampered-blob, each
    // said with its digest.
    let blobs = read_declared_blobs(layout, &image).map_err(tag_missing_and_tampered)?;

    let capsule_desc = image.subject.as_ref().ok_or_else(|| {
        PassportProblem::new(
            PassportProblemCode::SubjectMismatch,
            "the passport has no subject: it attests nothing",
        )
    })?;
    let capsule_digest = capsule_desc.digest.clone();

    // What the manifest declares about itself must be what the layout
    // holds. The annotations `hodei.passport.subject` and
    // `hodei.passport.vsa` name the two digests that make the passport
    // a record *of something*: a reader that did not check them would
    // accept a manifest pointing at one capsule while its layers
    // describe another. The check is cheap and it is the passport's
    // equivalent of the capsule's "the signature names this image".
    let declared_subject = image
        .annotations
        .get("hodei.passport.subject")
        .cloned()
        .unwrap_or_default();
    if declared_subject != capsule_digest {
        return Err(unsound(format!(
            "the passport annotation names subject {declared_subject}, its subject is {capsule_digest}"
        )));
    }

    // The VSA layer.
    let vsa_bytes = blob_of(&image, &blobs, VSA_MEDIA_TYPE, "vsa.json")?;
    let vsa: PassportVsa = serde_json::from_slice(vsa_bytes).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the VSA cannot be read: {error}"),
        )
    })?;
    if vsa.statement_type != IN_TOTO_STATEMENT_TYPE {
        return Err(PassportProblem::new(
            PassportProblemCode::Malformed,
            format!(
                "the VSA is not an in-toto statement: {}",
                vsa.statement_type
            ),
        ))?;
    }
    if vsa.predicate_type != VSA_PREDICATE_TYPE {
        return Err(PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the predicate is {}, not the VSA type", vsa.predicate_type),
        ))?;
    }
    // The VSA the annotation promises is the VSA the layers hold, and
    // the bundle the annotation names is the release layer byte for
    // byte. The record cannot be pointed at one document while being
    // read from another.
    let declared_vsa = image
        .annotations
        .get("hodei.passport.vsa")
        .cloned()
        .unwrap_or_default();
    let actual_vsa = digest_of(blob_of(&image, &blobs, VSA_MEDIA_TYPE, "vsa.json")?);
    if declared_vsa != actual_vsa {
        return Err(PassportProblem::new(
            PassportProblemCode::TamperedBlob,
            format!("the passport annotation names VSA {declared_vsa}, the VSA layer is {actual_vsa}"),
        ))?;
    }
    let declared_bundle = image
        .annotations
        .get("hodei.passport.bundle")
        .cloned()
        .unwrap_or_default();
    let actual_bundle = digest_of(blob_of(&image, &blobs, RELEASE_MEDIA_TYPE, "release.json")?);
    if declared_bundle != actual_bundle {
        return Err(PassportProblem::new(
            PassportProblemCode::TamperedBlob,
            format!(
                "the passport annotation names bundle {declared_bundle}, the release layer is {actual_bundle}"
            ),
        ))?;
    }
    // AC3 half one: the VSA's subject is the capsule, named exactly —
    // the same binding rule the chain applies to the provenance.
    let named = vsa
        .subject
        .first()
        .and_then(|subject| subject.digest.get("sha256"))
        .cloned()
        .unwrap_or_default();
    if named != hex_of(&capsule_digest) {
        return Err(PassportProblem::new(
            PassportProblemCode::SubjectMismatch,
            format!("the VSA speaks about sha256:{named}, the passport is about {capsule_digest}"),
        ))?;
    }

    // The policy the VSA digests must be the policy layer, and the
    // reader must be judging under exactly that policy.
    let policy_bytes = blob_of(&image, &blobs, POLICY_MEDIA_TYPE, "policy.json")?;
    let named_policy = vsa
        .predicate
        .policy
        .digest
        .get("sha256")
        .cloned()
        .unwrap_or_default();
    let actual_policy = hex_of(&digest_of(policy_bytes)).to_string();
    if named_policy != actual_policy {
        return Err(PassportProblem::new(
            PassportProblemCode::PolicyMismatch,
            format!(
                "the VSA names policy sha256:{named_policy}, the policy layer digests as {actual_policy}"
            ),
        ))?;
    }
    let layout_policy: PassportPolicy = serde_json::from_slice(policy_bytes).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the policy document cannot be read: {error}"),
        )
    })?;
    if &layout_policy != reader.policy {
        return Err(PassportProblem::new(
            PassportProblemCode::PolicyMismatch,
            "the policy the reader brings is not the policy the VSA names",
        ))?;
    }

    // The provenance envelope, from the bundle layer, canonical bytes.
    let bundle_bytes = blob_of(
        &image,
        &blobs,
        SIGSTORE_BUNDLE_MEDIA_TYPE,
        "provenance.bundle.json",
    )?;
    let envelope_bytes = envelope_from_bundle(bundle_bytes)?;
    let envelope_json = String::from_utf8(envelope_bytes.clone()).map_err(|_| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            "the envelope in the bundle is not UTF-8",
        )
    })?;
    // The VSA evaluated the envelope this layout holds — the bytes, not
    // another claim that merely verifies.
    let named_envelope = vsa
        .predicate
        .input_attestations
        .iter()
        .find(|attestation| attestation.uri == "provenance.bundle.json")
        .and_then(|attestation| attestation.digest.get("sha256"))
        .cloned()
        .unwrap_or_default();
    let actual_envelope = hex_of(&digest_of(&envelope_bytes)).to_string();
    if named_envelope != actual_envelope {
        return Err(PassportProblem::new(
            PassportProblemCode::VerdictMismatch,
            format!(
                "the VSA evaluated envelope sha256:{named_envelope}, the bundle holds {actual_envelope}"
            ),
        ))?;
    }
    // And the statement the VSA names is the statement layer that is
    // actually here, not a claim over bytes the layout never held.
    let named_statement = vsa
        .predicate
        .input_attestations
        .iter()
        .find(|attestation| attestation.uri == "provenance.json")
        .and_then(|attestation| attestation.digest.get("sha256"))
        .cloned()
        .unwrap_or_default();
    let statement_layer = image
        .layers
        .iter()
        .find(|layer| layer.media_type == PROVENANCE_STATEMENT_MEDIA_TYPE)
        .and_then(|layer| blobs.get(&layer.digest));
    let actual_statement = statement_layer
        .map(|bytes| hex_of(&digest_of(bytes)).to_string())
        .unwrap_or_default();
    if named_statement != actual_statement {
        return Err(PassportProblem::new(
            PassportProblemCode::VerdictMismatch,
            format!(
                "the VSA evaluated statement sha256:{named_statement}, the statement layer digests as {actual_statement}"
            ),
        ))?;
    }

    // Re-run the chain, the reader's way: the reader's roots, at the
    // reading moment, with the capsule as the bound subject. A key the
    // reader does not know is an unknown key — evidence about somebody
    // else's words, never a pass; the verdicts below settle what the
    // record is worth under these roots.
    let expected = ExpectedIdentity {
        subject_algorithm: "sha256".to_string(),
        subject_digest: hex_of(&capsule_digest).to_string(),
        issuer: reader.issuer.clone(),
        builder_id: reader.policy.trusted_builders.first().cloned(),
    };
    let chain = cosign_verify(&envelope_json, reader.roots, &expected, now)
        .map_err(|error| BundleError::Read(format!("the provenance cannot be read: {error}")))?;
    let root = match (&chain.root_lineage, chain.root_revision) {
        (Some(lineage), Some(revision)) => Some((lineage.clone(), revision)),
        _ => None,
    };
    // A signature over a revoked root holds cryptographically and is
    // refused by trust: the two facts stay separate in the refusal.
    if matches!(
        &chain.attestation.verification,
        AttestationVerification::Invalid {
            reason: SignatureFailure::Revoked { .. }
        }
    ) {
        return Err(PassportProblem::new(
            PassportProblemCode::SignerRevoked,
            "the provenance's signing key is revoked under the reader's roots at the reading moment",
        ))?;
    }

    let recomputed = assess_provenance(
        &chain,
        &expected,
        &SlsaExpectations {
            trusted_builder_ids: reader.policy.trusted_builders.clone(),
            pinned_issuer: reader.policy.pinned_issuer.clone(),
            builder_identity_proven: reader.policy.builder_identity_proven,
            hermetic_declared_complete: reader.policy.hermetic_declared_complete,
        },
    );
    let recorded = assessment_from_findings(
        &vsa.predicate.hodei_assessor_version,
        &vsa.predicate.hodei_findings,
    )?;

    // The anti-auto-affirmation gate: the record must say what the
    // bytes prove now. A requirement-by-requirement comparison over the
    // ladder's own model, plus the VSA's own result string.
    if verification_result(&recomputed) != vsa.predicate.verification_result {
        return Err(PassportProblem::new(
            PassportProblemCode::VerdictMismatch,
            format!(
                "the VSA says {}, the material verifies as {}",
                vsa.predicate.verification_result,
                verification_result(&recomputed)
            ),
        ))?;
    }
    if recorded.findings != recomputed.findings {
        let mut diff = Vec::new();
        for finding in &recomputed.findings {
            let prior = recorded
                .findings
                .iter()
                .find(|other| other.requirement == finding.requirement);
            if prior != Some(finding) {
                diff.push(format!(
                    "{}: recorded {:?}, material says {:?}",
                    requirement_name(finding.requirement),
                    prior.map(|f| &f.status),
                    &finding.status
                ));
            }
        }
        return Err(PassportProblem::new(
            PassportProblemCode::VerdictMismatch,
            format!(
                "the VSA's verdicts do not match the material: {}",
                diff.join("; ")
            ),
        ))?;
    }

    // The bundle's own bytes: the release layer is the bundle manifest
    // byte for byte, and the capsule's rule is re-applied — a
    // manipulated trust state cannot slip through the passport either.
    let release_bytes = blob_of(&image, &blobs, RELEASE_MEDIA_TYPE, "release.json")?;
    let manifest: BundleManifest = serde_json::from_slice(release_bytes).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the bundle manifest cannot be read: {error}"),
        )
    })?;
    manifest.check_importable()?;
    let capsule_layer = blob_of(&image, &blobs, TRUST_MEDIA_TYPE, "capsule.json")?;
    let trust_state: TrustState = serde_json::from_slice(capsule_layer).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the trust state cannot be read: {error}"),
        )
    })?;
    if TrustState::of(&manifest) != trust_state {
        return Err(unsound(
            "the capsule does not state the decisions the bundle manifest holds",
        ));
    }
    // AC3 half two: the origin and the temporal validity travel with
    // the passport, intact and attributable — the record says where and
    // when it came from even in an isolated import, with no service to
    // ask.
    if manifest.origin.account.is_empty() || trust_state.origin != manifest.origin {
        return Err(unsound("the passport lost the origin of its bundle"));
    }
    // The content itself: every asset blob the bundle cites must still
    // digest to what the release says. Evidence that is gone is
    // missing-evidence; evidence that lies is tampered-blob.
    for asset in &manifest.assets {
        let bytes = blobs.get(&asset.digest).ok_or_else(|| {
            PassportProblem::new(
                PassportProblemCode::MissingEvidence,
                format!(
                    "the blob {} of asset {} is cited by the release but absent from the passport",
                    asset.digest, asset.asset_path
                ),
            )
        })?;
        let computed = hodei_evidence::ContentDigest::of_bytes(bytes);
        if computed.to_string() != asset.digest || bytes.len() as u64 != asset.size_bytes {
            return Err(PassportProblem::new(
                PassportProblemCode::TamperedBlob,
                format!(
                    "the blob {} is not the content the release cites (it digests as {computed})",
                    asset.digest
                ),
            ))?;
        }
    }

    // The OpenPGP signature over the exact manifest bytes, the
    // capsule's pattern. The signature's referrer is NOT part of the
    // manifest identity — which is why AC4 can hold.
    let signature_image = read_manifest_of(layout, &index, PASSPORT_SIGNATURE_ARTIFACT_TYPE)
        .map_err(tag_missing_and_tampered)?;
    let subject = signature_image.subject.as_ref().ok_or_else(|| {
        PassportProblem::new(
            PassportProblemCode::SignatureInvalid,
            "the signature has no subject: it signs nothing",
        )
    })?;
    let signed_bytes = read_layout_blob(layout, subject).map_err(tag_missing_and_tampered)?;
    if serde_json::from_slice::<ImageManifest>(&signed_bytes)? != image {
        return Err(PassportProblem::new(
            PassportProblemCode::SignatureInvalid,
            "the signature subject does not name this passport",
        ))?;
    }
    let sig_desc = signature_image.layers.first().ok_or_else(|| {
        PassportProblem::new(
            PassportProblemCode::MissingEvidence,
            "the signature image carries no signature layer",
        )
    })?;
    let sig_blob = read_layout_blob(layout, sig_desc).map_err(tag_missing_and_tampered)?;
    let sig_json: serde_json::Value = serde_json::from_slice(&sig_blob).map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::Malformed,
            format!("the signature document cannot be read: {error}"),
        )
    })?;
    let signature = sig_json
        .get("signature")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let declared_signer = sig_json
        .get("signer")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    if declared_signer != signer.fingerprint_hex() {
        return Err(PassportProblem::new(
            PassportProblemCode::SignatureInvalid,
            format!(
                "the signature names key {declared_signer}, not the key the reader verifies with"
            ),
        ))?;
    }
    let public_key_asc = signer.key_asc().map_err(|error| {
        PassportProblem::new(
            PassportProblemCode::SignatureInvalid,
            format!("the signer has no public key: {error}"),
        )
    })?;
    MetadataSigner::verify_detached(&signed_bytes, signature.as_bytes(), &public_key_asc).map_err(
        |error| {
            PassportProblem::new(
                PassportProblemCode::SignatureInvalid,
                format!("the signature does not hold: {error}"),
            )
        },
    )?;

    // Freshness, the honest way. Offline by default: the reading knows
    // nothing about revocations newer than what its own roots carry,
    // and does not pretend otherwise — UnknownOffline is not "valid
    // today". With an explicit online policy, the age of the record is
    // measured against the reading moment: Stale past the threshold,
    // Fresh within — and even Fresh means only that the reader agreed
    // a freshness window; the record itself is never fresher than its
    // own timeVerified.
    let verified_at = DateTime::parse_from_rfc3339(&vsa.predicate.time_verified)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| {
            PassportProblem::new(
                PassportProblemCode::Malformed,
                format!("the VSA's timeVerified is not RFC 3339: {error}"),
            )
        })?;
    let freshness = match reader.online_freshness {
        None => FreshnessNow::UnknownOffline,
        Some(maximum_age) if now.signed_duration_since(verified_at) > maximum_age => {
            FreshnessNow::Stale
        }
        Some(_) => FreshnessNow::Fresh,
    };

    Ok(PassportReading {
        capsule_image_digest: capsule_digest,
        passport_image_digest: digest_of(&signed_bytes),
        manifest,
        trust_state,
        vsa,
        recorded,
        recomputed,
        cryptographic_holds: chain.cryptographic_holds,
        root,
        freshness,
        signer_fingerprint: declared_signer,
        verified_at,
    })
}

/// The blob of the layer with this media type, or `missing-evidence`.
fn blob_of<'a>(
    image: &ImageManifest,
    blobs: &'a BTreeMap<String, Vec<u8>>,
    media_type: &str,
    title: &str,
) -> Result<&'a Vec<u8>, PassportProblem> {
    let desc = image
        .layers
        .iter()
        .find(|layer| layer.media_type == media_type)
        .ok_or_else(|| {
            PassportProblem::new(
                PassportProblemCode::MissingEvidence,
                format!("the passport carries no {title} layer"),
            )
        })?;
    blobs.get(&desc.digest).ok_or_else(|| {
        PassportProblem::new(
            PassportProblemCode::MissingEvidence,
            format!("the {title} blob {} is declared but missing", desc.digest),
        )
    })
}

/// Tag the capsule's structural refusals with passport codes, so the
/// consumer's report distinguishes "the evidence is not there" from
/// "the evidence lies about itself".
fn tag_missing_and_tampered(error: BundleError) -> BundleError {
    match error {
        BundleError::Unsound(detail) => {
            if detail.contains("not there") {
                BundleError::Unsound(format!(
                    "[missing-evidence] {detail}: the record cannot be trusted over an absent blob"
                ))
            } else {
                BundleError::Unsound(format!("[tampered-blob] {detail}"))
            }
        }
        other => other,
    }
}

/// The SBOM layer of a passport layout, through the crate's own
/// CycloneDX 1.7 normalizer: the proof that the SBOM is a standard
/// document, not a private blob.
pub fn read_sbom(layout: &Path) -> Result<Sbom, BundleError> {
    let index: LayoutIndex = read_json(&layout.join(INDEX_FILE))?;
    let image = read_manifest_of(layout, &index, PASSPORT_ARTIFACT_TYPE)?;
    let blobs = read_declared_blobs(layout, &image).map_err(tag_missing_and_tampered)?;
    let bytes =
        blob_of(&image, &blobs, SBOM_MEDIA_TYPE, "sbom.cdx.json").map_err(BundleError::from)?;
    let text = String::from_utf8_lossy(bytes);
    read_cyclonedx_17(&text).map_err(|error| {
        BundleError::Unsound(format!("the SBOM cannot be read as CycloneDX 1.7: {error}"))
    })
}
