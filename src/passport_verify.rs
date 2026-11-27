//! Verification of a trust passport as a service (HSP-321 / HSP-160).
//!
//! `POST /api/v1/passports/verify` answers one question about a passport the
//! caller brings: *is this record sound, today, under the roots THIS
//! DEPLOYMENT trusts, and addressed to the caller's account?* It returns a
//! **verdict**. It grants nothing: no authorization path accepts a verdict, or
//! the passport, as a credential.
//!
//! # Where the trust comes from
//!
//! Every root of trust is fixed by the operator in a file the deployment
//! points at (`trust.passport_verification_file`). Nothing that decides the
//! verdict ever comes from the request, from the document, or from an
//! upstream: not the provenance roots, not the OpenPGP signer certificates,
//! not the policy, not the pinned issuer, not the validity windows. The
//! passport's own `trust-roots` layer is public documentation of what the
//! exporter used; the reader ignores it.
//!
//! # What is checked (each one fails closed)
//!
//! - the document is size-bounded (bytes, blobs) and strictly parsed
//!   (unknown members are refused, duplicate blobs are refused, digests must
//!   be `sha256:<64 hex>` so a name can never steer a file read);
//! - the passport is addressed to the caller: its origin account and
//!   repository are the caller's, checked **before** any verdict about the
//!   document is computed, so verification is never an oracle on somebody
//!   else's passport;
//! - every declared blob digests to what it declares; the manifest is in
//!   canonical form and the OpenPGP signature over those exact bytes holds
//!   under a *pinned* certificate, on an algorithm allow-list (Ed25519; hash
//!   SHA-256 or stronger — never "none", never SHA-1);
//! - the provenance DSSE envelope verifies (ECDSA P-256, the only algorithm
//!   the verifier implements: the algorithm is not selectable by the
//!   document) under a pinned root, which is not revoked and whose validity
//!   window contains the moment the passport was frozen;
//! - the VSA's verdicts are re-derived from the material, never believed;
//! - temporal validity: `nbf` (not before) = the freeze instant minus a
//!   bounded clock skew, `exp` = the freeze instant plus the operator's
//!   maximum age plus the skew;
//! - revocation: the promotion decisions the passport cites are asked of the
//!   trust ledger **now**; a decision that no longer backs its digest is
//!   `revoked`/`superseded`, and a ledger that cannot answer is a `503`,
//!   never a pass.
//!
//! # Replay
//!
//! The verification is a pure function of (document, roots, ledger, clock):
//! it keeps no state and consumes nothing, so repeating it is harmless and
//! gives the same verdict while those inputs do not change. The passport is a
//! record, not a single-use token, and no verdict is stored.

use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Duration, Utc};
use hodei_catalog::TrustLedger;
use hodei_evidence::standards::{RootRevision, SlsaBuildLevel, TrustRoots, VerificationKey};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::bundles::capsule_dgraph::{DecisionState, map_decision_state};
use crate::bundles::manifest::DecisionEvidence;
use crate::bundles::passport::{
    PassportPolicy, PassportProblemCode, PinnedSigner, ReaderExpectations, problem_code_of,
    read_passport_origin, verify_passport_pinned,
};
use crate::openpgp::MetadataSigner;

/// The format version of the operator's verification file.
pub const FILE_VERSION: u32 = 1;
/// The default ceiling of a passport document, in bytes (request body).
pub const DEFAULT_MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
/// The largest ceiling an operator may configure.
pub const HARD_MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
/// The most blobs a document may carry.
pub const MAX_BLOBS: usize = 512;
/// The most decisions of one passport that are asked of the ledger.
pub const MAX_LEDGER_LOOKUPS: usize = 256;
/// The most verifications that run at once.
pub const MAX_CONCURRENT: usize = 4;
/// How long a verification may take before it is given up as unavailable.
pub const VERIFY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long one ledger read may take.
pub const LEDGER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// The largest clock skew an operator may allow.
pub const MAX_SKEW_SECS: u64 = 300;
/// The largest maximum age an operator may allow (one year).
pub const MAX_AGE_SECS: u64 = 366 * 24 * 3600;
const MAX_ROOTS: usize = 32;

/// One provenance root as the operator writes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootEntry {
    pub lineage: String,
    pub revision: u32,
    /// The identifier a provenance signature names.
    pub key_id: String,
    /// The uncompressed ECDSA P-256 point, 130 hex characters (`04...`).
    pub public_point_hex: String,
    #[serde(default)]
    pub issuer: Option<String>,
    /// From when the revision may vouch for what was frozen.
    pub activated_at: DateTime<Utc>,
    /// After this instant the revision no longer vouches for new freezes.
    #[serde(default)]
    pub retired_at: Option<DateTime<Utc>>,
    /// From this instant the trust is taken back, whatever was frozen.
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One OpenPGP certificate that may sign passport manifests.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignerEntry {
    /// An operator label; never trusted for anything.
    pub id: String,
    pub public_key_armor: String,
    pub valid_from: DateTime<Utc>,
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
}

/// The operator's verification file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationFile {
    pub version: u32,
    /// The policy a passport must have been written under (byte-identical).
    pub policy: PassportPolicy,
    /// The issuer the provenance root must be pinned to, when pinned.
    #[serde(default)]
    pub issuer: Option<String>,
    /// How long after the freeze a passport stays valid, in seconds.
    pub max_age_secs: u64,
    /// The clock skew tolerated on both ends, in seconds (default 60).
    #[serde(default = "default_skew")]
    pub clock_skew_secs: u64,
    /// The lowest SLSA build level the record must assert (default 1).
    #[serde(default = "default_min_level")]
    pub min_slsa_level: u8,
    #[serde(default)]
    pub max_document_bytes: Option<usize>,
    pub provenance_roots: Vec<RootEntry>,
    pub signers: Vec<SignerEntry>,
}

fn default_skew() -> u64 {
    60
}

fn default_min_level() -> u8 {
    1
}

/// Why the file cannot be a verifier.
#[derive(Debug, thiserror::Error)]
#[error("the passport verification file is not usable: {0}")]
pub struct ConfigurationError(String);

fn invalid(what: impl Into<String>) -> ConfigurationError {
    ConfigurationError(what.into())
}

struct SignerPin {
    pinned: PinnedSigner,
    valid_from: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

/// The verifier a deployment mounts: its roots, fixed at boot.
pub struct PassportVerifier {
    roots: TrustRoots,
    revisions: Vec<RootRevision>,
    signers: Vec<SignerPin>,
    policy: PassportPolicy,
    issuer: Option<String>,
    max_age: Duration,
    skew: Duration,
    min_level: u8,
    max_document_bytes: usize,
    slots: Arc<Semaphore>,
}

impl std::fmt::Debug for PassportVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassportVerifier").finish_non_exhaustive()
    }
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

impl PassportVerifier {
    /// Reads and validates the operator's file. A file that is declared and
    /// not usable stops the boot: a verifier that silently trusts less, or
    /// more, than the operator wrote is worse than none.
    pub fn from_file(path: &str) -> Result<Self, ConfigurationError> {
        let text = std::fs::read_to_string(Path::new(path))
            .map_err(|error| invalid(format!("{path} cannot be read: {error}")))?;
        Self::from_json(&text)
    }

    pub fn from_json(text: &str) -> Result<Self, ConfigurationError> {
        let file: VerificationFile =
            serde_json::from_str(text).map_err(|error| invalid(error.to_string()))?;
        Self::from_file_model(file)
    }

    pub fn from_file_model(file: VerificationFile) -> Result<Self, ConfigurationError> {
        if file.version != FILE_VERSION {
            return Err(invalid(format!("version must be {FILE_VERSION}")));
        }
        if file.max_age_secs == 0 || file.max_age_secs > MAX_AGE_SECS {
            return Err(invalid("max_age_secs must be between 1 and one year"));
        }
        if file.clock_skew_secs > MAX_SKEW_SECS {
            return Err(invalid(format!(
                "clock_skew_secs must not exceed {MAX_SKEW_SECS}"
            )));
        }
        if !(1..=3).contains(&file.min_slsa_level) {
            return Err(invalid("min_slsa_level must be 1, 2 or 3"));
        }
        let max_document_bytes = file
            .max_document_bytes
            .unwrap_or(DEFAULT_MAX_DOCUMENT_BYTES);
        if max_document_bytes == 0 || max_document_bytes > HARD_MAX_DOCUMENT_BYTES {
            return Err(invalid("max_document_bytes is out of range"));
        }
        if file.provenance_roots.is_empty() || file.signers.is_empty() {
            return Err(invalid(
                "at least one provenance root and one signer must be pinned",
            ));
        }
        if file.provenance_roots.len() > MAX_ROOTS || file.signers.len() > MAX_ROOTS {
            return Err(invalid("too many pinned roots or signers"));
        }
        if file.policy.trusted_builders.is_empty() {
            return Err(invalid("the policy must trust at least one builder"));
        }

        let mut revisions = Vec::new();
        let mut roots = TrustRoots::new();
        let mut seen_keys = std::collections::BTreeSet::new();
        let mut seen_revisions = std::collections::BTreeSet::new();
        for entry in &file.provenance_roots {
            if entry.key_id.is_empty() || entry.key_id.len() > 128 || entry.lineage.is_empty() {
                return Err(invalid("a root has an empty or oversized identifier"));
            }
            if entry.revision == 0 {
                return Err(invalid("revisions start at 1"));
            }
            if !seen_keys.insert(entry.key_id.clone()) {
                return Err(invalid("a key id is pinned twice"));
            }
            if !seen_revisions.insert((entry.lineage.clone(), entry.revision)) {
                return Err(invalid("a lineage revision is pinned twice"));
            }
            let point = decode_hex(&entry.public_point_hex)
                .filter(|point| point.len() == 65 && point[0] == 0x04)
                .ok_or_else(|| invalid("a root point is not an uncompressed P-256 point"))?;
            if let Some(retired) = entry.retired_at
                && retired < entry.activated_at
            {
                return Err(invalid("a root is retired before it is activated"));
            }
            let revision = RootRevision {
                lineage: entry.lineage.clone(),
                revision: entry.revision,
                key: VerificationKey {
                    key_id: entry.key_id.clone(),
                    point,
                },
                issuer: entry.issuer.clone(),
                activated_at: entry.activated_at,
                retired_at: entry.retired_at,
                revoked_at: entry.revoked_at,
            };
            revisions.push(revision);
        }
        revisions.sort_by(|a, b| (&a.lineage, a.revision).cmp(&(&b.lineage, b.revision)));
        for revision in &revisions {
            roots.activate(
                revision.clone(),
                "operator",
                "pinned by the deployment configuration",
            );
        }

        let mut signers = Vec::new();
        let mut seen_fingerprints = std::collections::BTreeSet::new();
        for entry in &file.signers {
            let fingerprint = MetadataSigner::pinned_fingerprint(&entry.public_key_armor)
                .map_err(|error| invalid(format!("signer {}: {error}", entry.id)))?;
            if !seen_fingerprints.insert(fingerprint.clone()) {
                return Err(invalid("a signer certificate is pinned twice"));
            }
            if let Some(until) = entry.valid_until
                && until < entry.valid_from
            {
                return Err(invalid("a signer expires before it starts"));
            }
            signers.push(SignerPin {
                pinned: PinnedSigner {
                    fingerprint,
                    key_asc: entry.public_key_armor.clone(),
                },
                valid_from: entry.valid_from,
                valid_until: entry.valid_until,
                revoked_at: entry.revoked_at,
            });
        }

        Ok(Self {
            roots,
            revisions,
            signers,
            policy: file.policy,
            issuer: file.issuer,
            max_age: Duration::seconds(file.max_age_secs as i64),
            skew: Duration::seconds(file.clock_skew_secs as i64),
            min_level: file.min_slsa_level,
            max_document_bytes,
            slots: Arc::new(Semaphore::new(MAX_CONCURRENT)),
        })
    }

    /// The ceiling of a document, in bytes.
    pub fn max_document_bytes(&self) -> usize {
        self.max_document_bytes
    }

    /// Takes one of the verification slots, or `None` when all are busy.
    pub fn try_slot(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        self.slots.clone().try_acquire_owned().ok()
    }
}

/// The passport as it travels: the files of an OCI image layout.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassportDocument {
    /// The text of the `oci-layout` file.
    pub oci_layout: String,
    /// The text of `index.json`.
    pub index: String,
    /// The blobs, each under its own digest.
    pub blobs: Vec<DocumentBlob>,
}

/// One blob of the layout.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentBlob {
    /// `sha256:<64 lowercase hex>`.
    pub digest: String,
    /// The bytes, standard base64.
    pub data_base64: String,
}

/// A decision of the passport that must still back its digest.
#[derive(Debug, Clone)]
pub struct CitedDecision {
    pub digest: String,
    pub decision: DecisionEvidence,
}

/// A passport that is sound under the operator's roots, as of now, before the
/// revocation ledger is asked.
#[derive(Debug)]
pub struct Sound {
    pub capsule_image_digest: String,
    pub passport_image_digest: String,
    pub verified_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub slsa_result: String,
    pub root_lineage: String,
    pub root_revision: u32,
    pub signer_fingerprint: String,
    pub repository: String,
    pub cited: Vec<CitedDecision>,
}

/// A refusal with a stable code and a fixed explanation. The explanation
/// never repeats anything from the document, from the roots or from another
/// account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub explanation: &'static str,
}

const fn refusal(code: &'static str, explanation: &'static str) -> Refusal {
    Refusal { code, explanation }
}

/// Every refusal the verification can give.
pub mod refusals {
    use super::{Refusal, refusal};

    pub const MALFORMED: Refusal = refusal("malformed", "the document is not a readable passport");
    pub const MISSING_EVIDENCE: Refusal = refusal(
        "missing-evidence",
        "a blob the passport declares is not in the document",
    );
    pub const TAMPERED_BLOB: Refusal = refusal(
        "tampered-blob",
        "a blob does not digest to what the passport declares",
    );
    pub const POLICY_MISMATCH: Refusal = refusal(
        "policy-mismatch",
        "the passport was not written under the policy this deployment requires",
    );
    pub const VERDICT_MISMATCH: Refusal = refusal(
        "verdict-mismatch",
        "the verdicts the passport records are not what its material verifies to",
    );
    pub const SUBJECT_MISMATCH: Refusal = refusal(
        "subject-mismatch",
        "the passport does not speak about the capsule it names",
    );
    pub const SIGNATURE_INVALID: Refusal = refusal(
        "signature-invalid",
        "the signature over the passport does not hold under a key this deployment pins",
    );
    pub const AUDIENCE_MISMATCH: Refusal = refusal(
        "audience-mismatch",
        "the passport is not addressed to this account and repository",
    );
    pub const NOT_YET_VALID: Refusal = refusal(
        "not-yet-valid",
        "the passport is dated after the present moment",
    );
    pub const EXPIRED: Refusal = refusal(
        "expired",
        "the passport is older than this deployment accepts",
    );
    pub const VERIFICATION_FAILED: Refusal = refusal(
        "verification-failed",
        "the provenance does not verify under the roots this deployment pins, or is below the required level",
    );
    pub const SIGNER_REVOKED: Refusal = refusal(
        "signer-revoked",
        "the provenance signing key is revoked under this deployment's roots",
    );
    pub const SIGNER_NOT_VALID: Refusal = refusal(
        "signer-not-valid",
        "the passport signer is not valid for the moment the passport was frozen",
    );
    pub const ROOT_NOT_VALID: Refusal = refusal(
        "root-not-valid",
        "the provenance root was not valid for the moment the passport was frozen",
    );
    pub const REVOKED: Refusal = refusal(
        "revoked",
        "a promotion decision the passport cites no longer backs its digest",
    );
    pub const SUPERSEDED: Refusal = refusal(
        "superseded",
        "another decision now backs a digest the passport cites",
    );
    pub const TOO_LARGE: Refusal =
        refusal("too-large", "the passport exceeds the verification limits");
}

fn refusal_of_code(code: Option<PassportProblemCode>) -> Refusal {
    match code {
        Some(PassportProblemCode::MissingEvidence) => refusals::MISSING_EVIDENCE,
        Some(PassportProblemCode::TamperedBlob) => refusals::TAMPERED_BLOB,
        Some(PassportProblemCode::PolicyMismatch) => refusals::POLICY_MISMATCH,
        Some(PassportProblemCode::VerdictMismatch) => refusals::VERDICT_MISMATCH,
        Some(PassportProblemCode::SignerRevoked) => refusals::SIGNER_REVOKED,
        Some(PassportProblemCode::SubjectMismatch) => refusals::SUBJECT_MISMATCH,
        Some(PassportProblemCode::SignatureInvalid) => refusals::SIGNATURE_INVALID,
        Some(PassportProblemCode::Malformed) | None => refusals::MALFORMED,
    }
}

fn level_value(level: Option<SlsaBuildLevel>) -> u8 {
    match level {
        Some(SlsaBuildLevel::L1) => 1,
        Some(SlsaBuildLevel::L2) => 2,
        Some(SlsaBuildLevel::L3) => 3,
        None => 0,
    }
}

fn well_formed_digest(digest: &str) -> Option<&str> {
    digest.strip_prefix("sha256:").filter(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

impl PassportVerifier {
    /// Materializes the document as a layout in a fresh directory. Names are
    /// the validated digests only; nothing in the document chooses a path.
    fn materialize(&self, document: &PassportDocument) -> Result<tempfile::TempDir, Refusal> {
        if document.blobs.len() > MAX_BLOBS
            || document.oci_layout.len() > 4096
            || document.index.len() > self.max_document_bytes
        {
            return Err(refusals::TOO_LARGE);
        }
        let directory = tempfile::tempdir().map_err(|_| refusals::MALFORMED)?;
        let blobs = directory.path().join("blobs").join("sha256");
        std::fs::create_dir_all(&blobs).map_err(|_| refusals::MALFORMED)?;
        std::fs::write(directory.path().join("oci-layout"), &document.oci_layout)
            .map_err(|_| refusals::MALFORMED)?;
        std::fs::write(directory.path().join("index.json"), &document.index)
            .map_err(|_| refusals::MALFORMED)?;
        let mut seen = std::collections::BTreeSet::new();
        let mut total = 0usize;
        for blob in &document.blobs {
            let hex = well_formed_digest(&blob.digest).ok_or(refusals::MALFORMED)?;
            if !seen.insert(hex.to_string()) {
                return Err(refusals::MALFORMED);
            }
            // Decoded size is bounded before decoding: base64 is 4/3.
            if blob.data_base64.len() / 4 * 3 > self.max_document_bytes {
                return Err(refusals::TOO_LARGE);
            }
            let bytes = BASE64
                .decode(blob.data_base64.as_bytes())
                .map_err(|_| refusals::MALFORMED)?;
            total = total.saturating_add(bytes.len());
            if total > self.max_document_bytes {
                return Err(refusals::TOO_LARGE);
            }
            std::fs::write(blobs.join(hex), bytes).map_err(|_| refusals::MALFORMED)?;
        }
        Ok(directory)
    }

    /// The cryptographic and temporal verification. Synchronous and CPU
    /// bound: the caller runs it off the request task.
    pub fn verify_document(
        &self,
        document: &PassportDocument,
        account: &str,
        repository: &str,
        now: DateTime<Utc>,
    ) -> Result<Sound, Refusal> {
        let layout = self.materialize(document)?;
        let path = layout.path();

        // 1. Audience first: nothing about the validity of a passport that is
        // not the caller's is ever computed or said.
        let origin = read_passport_origin(path).map_err(|error| {
            tracing::debug!(%error, "the passport origin cannot be read");
            refusal_of_code(problem_code_of(&error))
        })?;
        if origin.account != account || origin.repository != repository {
            return Err(refusals::AUDIENCE_MISMATCH);
        }

        // 2. The reading under the operator's roots, signer and policy.
        let signers: Vec<PinnedSigner> = self.signers.iter().map(|s| s.pinned.clone()).collect();
        let reading = verify_passport_pinned(
            path,
            &signers,
            &ReaderExpectations {
                roots: &self.roots,
                policy: &self.policy,
                issuer: self.issuer.clone(),
                online_freshness: None,
            },
            now,
        )
        .map_err(|error| {
            tracing::debug!(%error, "the passport reading was refused");
            refusal_of_code(problem_code_of(&error))
        })?;
        if reading.manifest.origin != origin {
            return Err(refusals::AUDIENCE_MISMATCH);
        }

        // 3. The signer, under the operator's window for it.
        let pin = self
            .signers
            .iter()
            .find(|pin| {
                pin.pinned
                    .fingerprint
                    .eq_ignore_ascii_case(&reading.signer_fingerprint)
            })
            .ok_or(refusals::SIGNATURE_INVALID)?;
        let verified_at = reading.verified_at;
        if pin.revoked_at.is_some_and(|at| at <= now)
            || verified_at < pin.valid_from
            || pin.valid_until.is_some_and(|until| verified_at > until)
        {
            return Err(refusals::SIGNER_NOT_VALID);
        }

        // 4. Temporal validity: nbf and exp, with a bounded skew.
        if verified_at > now + self.skew || reading.manifest.exported_at > now + self.skew {
            return Err(refusals::NOT_YET_VALID);
        }
        let expires_at = verified_at + self.max_age;
        if now > expires_at + self.skew {
            return Err(refusals::EXPIRED);
        }

        // 5. The provenance must really verify under a pinned root, within
        // that root's window, at the level the operator demands. A record
        // that honestly says FAILED is a sound record of a failure, not a
        // valid passport.
        let (lineage, revision) = reading.root.clone().ok_or(refusals::VERIFICATION_FAILED)?;
        if !reading.cryptographic_holds
            || reading.vsa.predicate.verification_result == "FAILED"
            || level_value(reading.recomputed.asserted_level()) < self.min_level
        {
            return Err(refusals::VERIFICATION_FAILED);
        }
        let root = self
            .revisions
            .iter()
            .find(|candidate| candidate.lineage == lineage && candidate.revision == revision)
            .ok_or(refusals::VERIFICATION_FAILED)?;
        // The operator's issuer pin is enforced here, on the operator's own
        // root: the chain verification does not turn it into a refusal.
        for pin in [self.issuer.as_deref(), self.policy.pinned_issuer.as_deref()]
            .into_iter()
            .flatten()
        {
            if root.issuer.as_deref() != Some(pin) {
                return Err(refusals::VERIFICATION_FAILED);
            }
        }
        if root.is_revoked_at(now) {
            return Err(refusals::SIGNER_REVOKED);
        }
        if verified_at < root.activated_at || root.retired_at.is_some_and(|at| verified_at > at) {
            return Err(refusals::ROOT_NOT_VALID);
        }

        let cited: Vec<CitedDecision> = reading
            .manifest
            .assets
            .iter()
            .filter_map(|asset| {
                asset.decision.clone().map(|decision| CitedDecision {
                    digest: asset.digest.clone(),
                    decision,
                })
            })
            .collect();
        if cited.len() > MAX_LEDGER_LOOKUPS {
            return Err(refusals::TOO_LARGE);
        }

        Ok(Sound {
            capsule_image_digest: reading.capsule_image_digest,
            passport_image_digest: reading.passport_image_digest,
            verified_at,
            expires_at,
            slsa_result: reading.vsa.predicate.verification_result.clone(),
            root_lineage: lineage,
            root_revision: revision,
            signer_fingerprint: reading.signer_fingerprint,
            repository: origin.repository,
            cited,
        })
    }
}

/// The ledger could not answer: the verification has no verdict.
#[derive(Debug)]
pub struct LedgerUnavailable;

/// Asks the trust ledger, now, whether each cited decision still backs its
/// digest. `Ok(None)` is "all still in force"; any failure to ask is
/// [`LedgerUnavailable`], never a pass.
pub async fn check_revocation(
    ledger: &dyn TrustLedger,
    account: &str,
    sound: &Sound,
    now: DateTime<Utc>,
) -> Result<Option<Refusal>, LedgerUnavailable> {
    for cited in &sound.cited {
        let digest = {
            let hex = well_formed_digest(&cited.digest).ok_or(LedgerUnavailable)?;
            hodei_evidence::ContentDigest::sha256(hex)
        };
        let grant = tokio::time::timeout(
            LEDGER_TIMEOUT,
            ledger.active_grant(account, &sound.repository, &digest, now),
        )
        .await
        .map_err(|_| LedgerUnavailable)?
        .map_err(|error| {
            tracing::error!(%error, "the trust ledger could not be read for a passport");
            LedgerUnavailable
        })?;
        match map_decision_state(Some(&cited.decision), grant.as_ref()) {
            DecisionState::Active | DecisionState::NoDecision => {}
            DecisionState::Revoked => return Ok(Some(refusals::REVOKED)),
            DecisionState::Superseded { .. } => return Ok(Some(refusals::SUPERSEDED)),
        }
    }
    Ok(None)
}
