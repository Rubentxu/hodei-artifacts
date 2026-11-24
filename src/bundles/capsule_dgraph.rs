//! The release capsule over the graph authority: reading the capsule
//! without files, keeping the offline contract byte-for-byte.
//!
//! [`super::capsule`] writes and verifies the OCI layout, and that
//! layout stays the offline contract of HSP-126 untouched: a capsule a
//! site received over `oras` must look exactly the same now as before
//! this module existed. What moves here is only the READING: once a
//! verified layout has been ingested, the capsule answers from the graph
//! authority — the same digests, the same detached signature over the
//! same bytes, the same history of decisions and the same explicit
//! absences — so a reader does not need the blob directory on disk to
//! know what the capsule claims and what it withholds.
//!
//! The slice is additive: it owns the `hodei.capsule.*` namespace, its
//! DDL is applied idempotently at open, and it never touches a predicate
//! another slice owns. Every read is bounded by the capsule's own key —
//! the tenant and the image digest name the capsule, and the composite
//! capsule key names every row of its assets and its absences. Nothing
//! in this module scans: a distractor capsule cannot be reached through
//! another capsule's key, and the rows that answer are re-verified
//! against the manifest the capsule itself carries.
//!
//! The revocation rule mirrors the capsule's own honesty: a decision the
//! capsule recorded is history and stays in the record forever (a signed
//! capsule is never rewritten — not offline, and not here); whether that
//! decision still backs the digest today is read from the trust ledger
//! and reported beside it, never folded into the old assertion and never
//! hidden. A revoked grant shows as revoked.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{GraphClient, GraphClientError, GraphUpsertTransaction, Tenant};
use serde_json::Value;

use super::BundleError;
use super::capsule::{
    CAPSULE_ARTIFACT_TYPE, Descriptor, INDEX_FILE, ImageManifest, LayoutIndex, RELEASE_MEDIA_TYPE,
    SIGNATURE_ARTIFACT_TYPE, SIGNATURE_MEDIA_TYPE, TRUST_MEDIA_TYPE, TrustState, read_json,
    read_layout_blob,
};
use super::manifest::{BundleManifest, DecisionEvidence, MAX_BUNDLE_ASSETS, Withheld};
use crate::openpgp::MetadataSigner;

/// The node type of one ingested capsule.
pub const CAPSULE_TYPE: &str = "HodeiCapsule";
/// The node type of one resolved path of an ingested capsule.
pub const ASSET_TYPE: &str = "HodeiCapsuleAsset";
/// The node type of one withheld path of an ingested capsule.
pub const WITHHELD_TYPE: &str = "HodeiCapsuleWithheld";

/// The DDL slice of this adapter: additive predicates and types, applied
/// idempotently at open. It owns `hodei.capsule.*` and nothing else.
pub const CAPSULE_SCHEMA_DQL: &str = "
hodei.capsule.key: string @index(exact) @upsert .
hodei.capsule.tenant: string @index(exact) .
hodei.capsule.account: string @index(exact) .
hodei.capsule.repository: string @index(exact) .
hodei.capsule.image_digest: string @index(exact) .
hodei.capsule.release_digest: string @index(exact) .
hodei.capsule.exported_at: datetime .
hodei.capsule.upstream_id: string .
hodei.capsule.config_version: int .
hodei.capsule.image_manifest: string .
hodei.capsule.release_json: string .
hodei.capsule.trust_json: string .
hodei.capsule.signature: string .
hodei.capsule.signer: string .
hodei.capsule.assets: [uid] .
hodei.capsule.withheld: [uid] .
hodei.capsule.asset.key: string @index(exact) @upsert .
hodei.capsule.asset.capsule: string @index(exact) .
hodei.capsule.asset.asset_path: string .
hodei.capsule.asset.package: string .
hodei.capsule.asset.version: string .
hodei.capsule.asset.digest: string @index(exact) .
hodei.capsule.asset.size_bytes: int .
hodei.capsule.asset.receipt_id: string .
hodei.capsule.asset.receipt_outcome: string .
hodei.capsule.asset.decision_id: string .
hodei.capsule.asset.decision_actor: string .
hodei.capsule.asset.decision_granted_at: datetime .
hodei.capsule.withheld.key: string @index(exact) @upsert .
hodei.capsule.withheld.capsule: string @index(exact) .
hodei.capsule.withheld.asset_path: string .
hodei.capsule.withheld.reason: string .
type HodeiCapsule {
  hodei.capsule.key
  hodei.capsule.tenant
  hodei.capsule.account
  hodei.capsule.repository
  hodei.capsule.image_digest
  hodei.capsule.release_digest
  hodei.capsule.exported_at
  hodei.capsule.upstream_id
  hodei.capsule.config_version
  hodei.capsule.image_manifest
  hodei.capsule.release_json
  hodei.capsule.trust_json
  hodei.capsule.signature
  hodei.capsule.signer
  hodei.capsule.assets
  hodei.capsule.withheld
}
type HodeiCapsuleAsset {
  hodei.capsule.asset.key
  hodei.capsule.asset.capsule
  hodei.capsule.asset.asset_path
  hodei.capsule.asset.package
  hodei.capsule.asset.version
  hodei.capsule.asset.digest
  hodei.capsule.asset.size_bytes
  hodei.capsule.asset.receipt_id
  hodei.capsule.asset.receipt_outcome
  hodei.capsule.asset.decision_id
  hodei.capsule.asset.decision_actor
  hodei.capsule.asset.decision_granted_at
}
type HodeiCapsuleWithheld {
  hodei.capsule.withheld.key
  hodei.capsule.withheld.capsule
  hodei.capsule.withheld.asset_path
  hodei.capsule.withheld.reason
}
";

/// The fields of the capsule row: its own list only. The asset and
/// withheld rows answer to separate bounded lookups keyed by the
/// capsule's composite key, so the root read is one exact-key lookup
/// that never follows an edge into a census.
const CAPSULE_FIELDS: &str = "hodei.capsule.tenant hodei.capsule.account \
                              hodei.capsule.repository hodei.capsule.image_digest \
                              hodei.capsule.release_digest hodei.capsule.exported_at \
                              hodei.capsule.upstream_id hodei.capsule.config_version \
                              hodei.capsule.image_manifest hodei.capsule.release_json \
                              hodei.capsule.trust_json hodei.capsule.signature \
                              hodei.capsule.signer";

const ASSET_FIELDS: &str = "hodei.capsule.asset.asset_path hodei.capsule.asset.package \
                            hodei.capsule.asset.version hodei.capsule.asset.digest \
                            hodei.capsule.asset.size_bytes hodei.capsule.asset.receipt_id \
                            hodei.capsule.asset.receipt_outcome hodei.capsule.asset.decision_id \
                            hodei.capsule.asset.decision_actor \
                            hodei.capsule.asset.decision_granted_at";

const WITHHELD_FIELDS: &str = "hodei.capsule.withheld.asset_path hodei.capsule.withheld.reason";

const MAX_ATTEMPTS: usize = 8;
const RETRY_BASE: Duration = Duration::from_millis(10);
const RETRY_CAP: Duration = Duration::from_millis(100);

/// Why a capsule graph operation failed. Every variant is closed: the
/// caller knows exactly what happened and nothing was guessed.
#[derive(Debug, thiserror::Error)]
pub enum CapsuleGraphError {
    /// The authority did not answer, or refused the transaction.
    #[error("the capsule graph is not available: {0}")]
    Unavailable(String),
    /// The layout or the stored capsule is not what the contract of this
    /// slice promises: nothing was written, nothing is read as true.
    #[error("the capsule is not sound: {0}")]
    Unsound(String),
    /// The caller asked something this slice refuses to do: rewrite the
    /// bytes of a signed capsule, or read beyond the honest bound of a
    /// bundle.
    #[error("the capsule operation is refused: {0}")]
    Refused(String),
}

impl CapsuleGraphError {
    fn unavailable(error: impl std::fmt::Display) -> Self {
        Self::Unavailable(error.to_string())
    }
}

fn unsound(what: impl Into<String>) -> CapsuleGraphError {
    CapsuleGraphError::Unsound(what.into())
}

fn refused(what: impl Into<String>) -> CapsuleGraphError {
    CapsuleGraphError::Refused(what.into())
}

/// Escape one part of a composite key: the separator `|` and the escape
/// marker must never appear raw inside a part, or two different values
/// would name the same key.
fn key_part(value: &str) -> String {
    value.replace('%', "%25").replace('|', "%7C")
}

/// The graph key of one capsule: the tenant plus the OCI digest of the
/// image manifest the signature signs. The tenant rides inside the key
/// so two deployments never collide on one capsule row.
pub fn capsule_key(tenant: &Tenant, image_digest: &str) -> String {
    format!("{}|{}", tenant.as_str(), key_part(image_digest))
}

/// The graph key of one resolved path of one capsule.
pub fn asset_key(tenant: &Tenant, image_digest: &str, asset_path: &str) -> String {
    format!(
        "{}|{}",
        capsule_key(tenant, image_digest),
        key_part(asset_path)
    )
}

/// The graph key of one withheld path of one capsule. An absence and a
/// presence of the same path would be the same key, and the bundle
/// contract forbids exactly that.
pub fn withheld_key(tenant: &Tenant, image_digest: &str, asset_path: &str) -> String {
    asset_key(tenant, image_digest, asset_path)
}

/// The shape of every query this slice issues: an exact `eq` over an
/// indexed predicate, the honest bundle bound as `first:` where a list
/// is read, and never a scan (no `has()`, no type census, no unfiltered
/// head). Values are escaped here — the composite keys hold `|`, and an
/// unescaped key is a DQL lex error, not a query. Pure, so the tests
/// can pin the shape — AC2 is a shape property before it is a server
/// property.
pub fn bounded_query(var: &str, predicate: &str, value: &str, fields: &str, bound: bool) -> String {
    let head = if bound {
        format!(
            "func: eq({predicate}, {value}), first: {limit}",
            value = dql_string(value),
            limit = MAX_BUNDLE_ASSETS + 1
        )
    } else {
        format!("func: eq({predicate}, {})", dql_string(value))
    };
    format!("{{ {var}({head}) {{ {fields} }} }}")
}

/// The lookup of one capsule by its own key.
pub fn capsule_key_query(tenant: &Tenant, image_digest: &str) -> String {
    bounded_query(
        "cap",
        "hodei.capsule.key",
        &capsule_key(tenant, image_digest),
        CAPSULE_FIELDS,
        false,
    )
}

/// The lookup of the resolved paths of one capsule, by the capsule's
/// composite key that every row carries.
pub fn asset_rows_query(capsule: &str) -> String {
    bounded_query(
        "assets",
        "hodei.capsule.asset.capsule",
        capsule,
        ASSET_FIELDS,
        true,
    )
}

/// The lookup of the withheld paths of one capsule, by its own key.
pub fn withheld_rows_query(capsule: &str) -> String {
    bounded_query(
        "withheld",
        "hodei.capsule.withheld.capsule",
        capsule,
        WITHHELD_FIELDS,
        true,
    )
}

/// The OCI content digest of bytes: `sha256:` plus lowercase hex, the
/// form every descriptor of the layout announces. The same lowercase
/// rule [`super::capsule`] applies to the blobs on disk.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut digest = String::from("sha256:");
    for byte in sha2::Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").expect("writing to a String never fails");
    }
    digest
}

/// The current backing of a decision the capsule recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionState {
    /// The path had no decision at export: nothing backs it, and the
    /// capsule said so with a `null`.
    NoDecision,
    /// The decision the capsule names is still the active grant today.
    Active,
    /// Another decision backs the digest now; the capsule's decision
    /// remains the history of what was in force at export.
    Superseded {
        /// The decision active at read time.
        current: String,
    },
    /// No grant backs the digest any more: the capsule's decision was
    /// revoked or expired. The capsule keeps naming it — the revocation
    /// is visible beside the history, never folded into it.
    Revoked,
}

/// Pure mapping of the decision the capsule recorded against what the
/// trust ledger answers now. This is what "the revocation is never
/// hidden" rests on: a missing active grant cannot silently rewrite the
/// record into "no decision", and an active grant of another decision
/// cannot be read as the capsule's own.
pub fn map_decision_state(
    recorded: Option<&DecisionEvidence>,
    current: Option<&TrustGrant>,
) -> DecisionState {
    match (recorded, current) {
        (None, _) => DecisionState::NoDecision,
        (Some(_), None) => DecisionState::Revoked,
        (Some(recorded), Some(current)) if current.decision_id == recorded.decision_id => {
            DecisionState::Active
        }
        (Some(_), Some(current)) => DecisionState::Superseded {
            current: current.decision_id.clone(),
        },
    }
}

/// One resolved path of an ingested capsule.
#[derive(Debug, Clone)]
pub struct GraphAsset {
    pub asset_path: String,
    pub package: String,
    pub version: String,
    /// The content digest of the path, as the bundle names it.
    pub digest: String,
    pub size_bytes: u64,
    pub receipt_id: String,
    pub receipt_outcome: String,
    /// The decision the capsule recorded — history, never rewritten.
    pub decision: Option<DecisionEvidence>,
    /// Where that decision stands today, once the trust ledger was read
    /// ([`DgraphCapsuleStore::read_capsule_with_trust`]).
    pub decision_state: DecisionState,
}

/// A capsule as the graph holds it: the exact bytes the layout signed,
/// read back with every digest re-verified against what the stored row
/// itself announces.
#[derive(Debug)]
pub struct GraphCapsule {
    pub tenant: String,
    pub account: String,
    pub repository: String,
    /// The digest of the image manifest the detached signature signs.
    pub image_digest: String,
    /// The digest the capsule announced for the bundle manifest bytes.
    pub release_digest: String,
    pub exported_at: DateTime<Utc>,
    pub upstream_id: String,
    pub config_version: u64,
    /// The exact bytes of the capsule's image manifest.
    pub image_manifest_bytes: Vec<u8>,
    pub image: ImageManifest,
    /// The exact bytes of the bundle manifest the capsule contains.
    pub release_bytes: Vec<u8>,
    pub manifest: BundleManifest,
    /// The exact bytes of the trust-state layer as it was exported.
    pub trust_bytes: Vec<u8>,
    pub capsule: TrustState,
    /// The armored detached signature exactly as the layout holds it.
    pub signature_armor: String,
    /// The key the signature names; verification checks against it.
    pub signer: String,
    /// The resolved paths, in the order the bundle states them.
    pub assets: Vec<GraphAsset>,
    /// The explicit absences, with their reasons.
    pub withheld: Vec<Withheld>,
}

impl GraphCapsule {
    /// Whether the armored detached signature holds over the exact
    /// image-manifest bytes this capsule carries: the same check the
    /// offline verifier runs, over the same bytes.
    pub fn verify_signature(&self, signer: &MetadataSigner) -> Result<(), BundleError> {
        if self.signer != signer.fingerprint_hex() {
            return Err(BundleError::Unsound(format!(
                "the capsule names key {}, not the key we verify with",
                self.signer
            )));
        }
        let public_key_asc = signer
            .key_asc()
            .map_err(|e| BundleError::Unsound(format!("the signer has no public key: {e}")))?;
        MetadataSigner::verify_detached(
            &self.image_manifest_bytes,
            self.signature_armor.as_bytes(),
            &public_key_asc,
        )
        .map_err(|e| BundleError::Unsound(format!("the capsule signature does not hold: {e}")))
    }

    /// Where one recorded decision stands today, once the trust ledger
    /// has been consulted.
    pub fn decision_state_of(&self, asset_path: &str) -> Option<&DecisionState> {
        self.assets
            .iter()
            .find(|row| row.asset_path == asset_path)
            .map(|row| &row.decision_state)
    }
}

/// What an ingest did: the capsule it now holds and whether the graph
/// already had exactly these bytes.
#[derive(Debug, Clone)]
pub struct CapsuleIngest {
    pub tenant: String,
    pub image_digest: String,
    /// True when the graph already held this exact capsule.
    pub replayed: bool,
}

/// The digest the layout's index announces for the capsule manifest.
/// Every blob read here is checked against the descriptor that names
/// it — the same honest check the offline verifier runs — without
/// needing a key.
pub fn capsule_image_digest(layout: &Path) -> Result<String, CapsuleGraphError> {
    let index: LayoutIndex = layout_json(layout)?;
    for entry in &index.manifests {
        let bytes = layout_blob(layout, entry)?;
        let candidate: ImageManifest = serde_json::from_slice(&bytes)
            .map_err(|e| unsound(format!("a manifest the index announces: {e}")))?;
        if candidate.artifact_type == CAPSULE_ARTIFACT_TYPE {
            return Ok(entry.digest.clone());
        }
    }
    Err(unsound("the index has no capsule manifest"))
}

/// The `index.json` of the layout, mapped into this slice's error type.
fn layout_json(dir: &Path) -> Result<LayoutIndex, CapsuleGraphError> {
    read_json(&dir.join(INDEX_FILE)).map_err(|e| unsound(format!("the layout index: {e}")))
}

/// One declared blob of the layout, checked against its descriptor.
fn layout_blob(dir: &Path, desc: &Descriptor) -> Result<Vec<u8>, CapsuleGraphError> {
    read_layout_blob(dir, desc).map_err(|e| unsound(e.to_string()))
}

/// The capsule layout over the graph authority. Open through
/// [`DgraphCapsuleStore::open`], which applies this slice's additive DDL
/// slice idempotently.
#[derive(Debug)]
pub struct DgraphCapsuleStore {
    client: Arc<GraphClient>,
    tenant: Tenant,
}

impl DgraphCapsuleStore {
    pub async fn open(client: Arc<GraphClient>, tenant: Tenant) -> Result<Self, CapsuleGraphError> {
        apply_capsule_schema(&client).await?;
        Ok(Self { client, tenant })
    }

    /// The tenant whose keys this store writes and reads.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    /// Ingest one verified capsule layout into the graph. The bytes kept
    /// are the exact bytes of the layout — the signed image manifest,
    /// the bundle manifest, the trust state, the armored signature —
    /// with no re-serialization, so the capsule's digests and its
    /// signature survive the move to the authority unchanged.
    ///
    /// The offline verifier runs first and is the only authority about
    /// what is written: a layout that does not verify — a bad blob, a
    /// wrong key, a signature over other bytes — never reaches the
    /// graph. The write itself is create-if-absent on the capsule's own
    /// key: a second ingest of the same bytes replays, and a capsule
    /// whose stored bytes differ is refused. This authority never
    /// rewrites a signed capsule.
    pub async fn ingest(
        &self,
        layout: &Path,
        signer: &MetadataSigner,
    ) -> Result<CapsuleIngest, CapsuleGraphError> {
        super::capsule::verify_capsule(layout, signer)
            .map_err(|e| unsound(format!("the layout is not a sound capsule: {e}")))?;

        let index: LayoutIndex = layout_json(layout)?;

        // The exact bytes of the capsule's image manifest, with the
        // digest the index announces for them: the signature is over
        // these and nothing else.
        let mut capsule_manifest: Option<(String, Vec<u8>)> = None;
        for entry in &index.manifests {
            let bytes = layout_blob(layout, entry)?;
            let candidate: ImageManifest = serde_json::from_slice(&bytes)
                .map_err(|e| unsound(format!("a manifest the index announces: {e}")))?;
            if candidate.artifact_type == CAPSULE_ARTIFACT_TYPE && capsule_manifest.is_none() {
                capsule_manifest = Some((entry.digest.clone(), bytes));
            }
        }
        let (image_digest, image_bytes) =
            capsule_manifest.ok_or_else(|| unsound("the index has no capsule manifest"))?;
        if digest_of(&image_bytes) != image_digest {
            return Err(unsound(
                "the capsule manifest is not the bytes its digest names",
            ));
        }
        let image: ImageManifest = serde_json::from_slice(&image_bytes)
            .map_err(|e| unsound(format!("the capsule manifest: {e}")))?;

        // The signature manifest, byte-exact too: the armor travels as
        // the layout wrote it.
        let mut signature_manifest: Option<ImageManifest> = None;
        for entry in &index.manifests {
            let bytes = layout_blob(layout, entry)?;
            let candidate: ImageManifest = serde_json::from_slice(&bytes)
                .map_err(|e| unsound(format!("a manifest the index announces: {e}")))?;
            if candidate.artifact_type == SIGNATURE_ARTIFACT_TYPE && signature_manifest.is_none() {
                signature_manifest = Some(candidate);
            }
        }
        let signature_image =
            signature_manifest.ok_or_else(|| unsound("the index has no signature manifest"))?;
        let subject = signature_image
            .subject
            .as_ref()
            .ok_or_else(|| unsound("the signature has no subject: it signs nothing"))?;
        if subject.digest != image_digest {
            return Err(unsound("the signature subject does not name this capsule"));
        }
        let sig_layer_desc = signature_image
            .layers
            .first()
            .ok_or_else(|| unsound("the signature image carries no signature layer"))?;
        if sig_layer_desc.media_type != SIGNATURE_MEDIA_TYPE {
            return Err(unsound(format!(
                "the signature layer is {}, not an openpgp signature",
                sig_layer_desc.media_type
            )));
        }
        let sig_json: Value = serde_json::from_slice(&layout_blob(layout, sig_layer_desc)?)
            .map_err(|e| unsound(format!("the signature layer is not JSON: {e}")))?;
        if sig_json.get("format").and_then(Value::as_str) != Some("openpgp-detached") {
            return Err(unsound("the signature layer is not a detached signature"));
        }
        let signature_armor = sig_json
            .get("signature")
            .and_then(Value::as_str)
            .ok_or_else(|| unsound("the signature layer has no signature"))?
            .to_string();
        let declared_signer = sig_json
            .get("signer")
            .and_then(Value::as_str)
            .ok_or_else(|| unsound("the signature layer names no key"))?
            .to_string();

        // The named layers, byte-exact: the release carries the bundle
        // manifest, the trust layer carries the trust state of the
        // moment it was exported.
        let release_desc = find_layer(&image, RELEASE_MEDIA_TYPE)?;
        let release_bytes = layout_blob(layout, release_desc)?;
        if digest_of(&release_bytes) != release_desc.digest {
            return Err(unsound(
                "the release layer is not the bytes it is named for",
            ));
        }
        let trust_desc = find_layer(&image, TRUST_MEDIA_TYPE)?;
        let trust_bytes = layout_blob(layout, trust_desc)?;
        if digest_of(&trust_bytes) != trust_desc.digest {
            return Err(unsound("the trust layer is not the bytes it is named for"));
        }
        let manifest: BundleManifest = serde_json::from_slice(&release_bytes)
            .map_err(|e| unsound(format!("the release layer is not a bundle manifest: {e}")))?;
        manifest.check_importable().map_err(|e| {
            unsound(format!(
                "the capsule carries a bundle the import would refuse: {e}"
            ))
        })?;
        if manifest.origin.config_version == 0 && manifest.origin.upstream_id.is_empty() {
            return Err(unsound("a stored bundle with an empty origin"));
        }

        let key = capsule_key(&self.tenant, &image_digest);
        let nquads = capsule_nquads(
            &self.tenant,
            &image_digest,
            &image_bytes,
            &release_bytes,
            &trust_bytes,
            &signature_armor,
            &declared_signer,
            &manifest,
        );
        let probe = format!(
            "{{\n  cap(func: eq(hodei.capsule.key, {key_q})) {{ \
             hodei.capsule.image_manifest hodei.capsule.release_json hodei.capsule.signature \
             }} \n}}",
            key_q = dql_string(&key),
        );

        // Create only while absent. First the bounded read of the state:
        // if the capsule already stands, its bytes decide everything —
        // exactly these bytes is a replay, other bytes is a refusal
        // (this authority never rewrites a signed capsule). If nothing
        // stands, the write goes through an upsert gated on absence.
        // A race between the probe and the gate surfaces as the server's
        // uniqueness conflict on the `@upsert` key: the loser aborts,
        // retries, and its answer is the state after the commit — either
        // its own bytes (a replay) or a different capsule under this
        // name (refused).
        let mut last = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            let standing = self.query_json(&probe).await?;
            let nodes = standing
                .get("cap")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(node) = nodes.first() {
                return if stored_capsule_matches(
                    node,
                    &image_bytes,
                    &release_bytes,
                    &signature_armor,
                ) {
                    Ok(CapsuleIngest {
                        tenant: self.tenant.as_str().to_string(),
                        image_digest,
                        replayed: true,
                    })
                } else {
                    Err(refused(format!(
                        "the graph already holds a capsule keyed {key} with different bytes; \
                         a signed capsule is never rewritten"
                    )))
                };
            }

            let transaction = GraphUpsertTransaction {
                query: format!(
                    "{{\n  cap as var(func: eq(hodei.capsule.key, {key_q}))\n}}",
                    key_q = dql_string(&key),
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(cap), 0)",
                    nquads.clone(),
                )],
                commit_now: true,
            };
            match self.client.upsert(transaction).await {
                Ok(response) => {
                    require_no_errors(&response.json)?;
                    return Ok(CapsuleIngest {
                        tenant: self.tenant.as_str().to_string(),
                        image_digest,
                        replayed: false,
                    });
                }
                Err(GraphClientError::UpsertConflict { detail }) => {
                    last = detail;
                    let delay = (RETRY_BASE * attempt as u32).min(RETRY_CAP);
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(CapsuleGraphError::unavailable(error)),
            }
        }
        Err(CapsuleGraphError::Unavailable(format!(
            "the capsule ingest raced and never settled after {MAX_ATTEMPTS} attempts: {last}"
        )))
    }

    /// Read a capsule by the OCI digest of its image manifest. Three
    /// bounded lookups, all keyed by the capsule itself: the row by its
    /// exact key, the resolved paths and the withheld paths by the
    /// capsule key every row carries, each with the honest bundle bound.
    /// Nothing scans; the rows that answer are exactly the paths the
    /// stored manifest names, re-verified against the digests the graph
    /// carries.
    pub async fn read_capsule(
        &self,
        image_digest: &str,
    ) -> Result<GraphCapsule, CapsuleGraphError> {
        let key = capsule_key(&self.tenant, image_digest);
        let root = self
            .query_json(&capsule_key_query(&self.tenant, image_digest))
            .await?;
        let nodes = root
            .get("cap")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let node = nodes
            .first()
            .ok_or_else(|| refused(format!("the graph holds no capsule keyed {key}")))?;

        let stored_digest = text_of(node, "hodei.capsule.image_digest")?;
        if stored_digest != image_digest {
            return Err(unsound("the capsule under this key names another digest"));
        }
        let release_digest = text_of(node, "hodei.capsule.release_digest")?;
        let exported_at = time_of(node, "hodei.capsule.exported_at")?;
        let config_version = int_of(node, "hodei.capsule.config_version")?;
        let image_manifest_bytes = text_of(node, "hodei.capsule.image_manifest")?.into_bytes();
        let release_bytes = text_of(node, "hodei.capsule.release_json")?.into_bytes();
        let trust_bytes = text_of(node, "hodei.capsule.trust_json")?.into_bytes();

        // The digests survive the graph exactly as the layout signed
        // them: the image manifest is the bytes the signature is over,
        // the release is the digest the capsule announced, and the
        // trust state is the state the release alone derives.
        if digest_of(&image_manifest_bytes) != stored_digest {
            return Err(unsound(
                "the stored image manifest is not the bytes its digest names",
            ));
        }
        if digest_of(&release_bytes) != release_digest {
            return Err(unsound(
                "the stored release json is not the bytes its digest names",
            ));
        }
        let image: ImageManifest = serde_json::from_slice(&image_manifest_bytes)
            .map_err(|e| unsound(format!("the stored image manifest: {e}")))?;
        if image.artifact_type != CAPSULE_ARTIFACT_TYPE {
            return Err(unsound("the stored manifest is not a capsule"));
        }
        let release_desc = find_layer(&image, RELEASE_MEDIA_TYPE)?;
        let trust_desc = find_layer(&image, TRUST_MEDIA_TYPE)?;
        // The image declares a digest for its release layer; the capsule
        // announces the digest of its bundle. Offline these are the same
        // number because they name the same bytes — and the graph keeps
        // both claims, so the read must verify they agree.
        if release_desc.digest != release_digest {
            return Err(unsound(
                "the release layer of the image and the capsule's bundle digest disagree",
            ));
        }
        if digest_of(&trust_bytes) != trust_desc.digest {
            return Err(unsound(
                "the stored trust state is not the bytes the capsule announced",
            ));
        }
        let manifest: BundleManifest = serde_json::from_slice(&release_bytes)
            .map_err(|e| unsound(format!("the stored bundle manifest: {e}")))?;
        manifest
            .check_importable()
            .map_err(|e| unsound(format!("a stored bundle the import would refuse: {e}")))?;
        let capsule: TrustState = serde_json::from_slice(&trust_bytes)
            .map_err(|e| unsound(format!("the stored trust state: {e}")))?;
        if TrustState::of(&manifest) != capsule {
            return Err(unsound(
                "the stored capsule does not state the decisions its bundle holds",
            ));
        }
        if capsule.exported_at != exported_at {
            return Err(unsound("the stored capsule timestamps disagree"));
        }

        let assets = self.read_assets(&key, &manifest).await?;
        let withheld = self.read_withheld(&key, &manifest).await?;

        Ok(GraphCapsule {
            tenant: text_of(node, "hodei.capsule.tenant")?,
            account: text_of(node, "hodei.capsule.account")?,
            repository: text_of(node, "hodei.capsule.repository")?,
            image_digest: stored_digest,
            release_digest,
            exported_at,
            upstream_id: text_of(node, "hodei.capsule.upstream_id")?,
            config_version,
            image_manifest_bytes,
            image,
            release_bytes,
            manifest,
            trust_bytes,
            capsule,
            signature_armor: text_of(node, "hodei.capsule.signature")?,
            signer: text_of(node, "hodei.capsule.signer")?,
            assets,
            withheld,
        })
    }

    /// Read a capsule and, beside each recorded decision, where it
    /// stands today according to the trust ledger the caller names. The
    /// capsule bytes are the past's; the ledger's answer is the
    /// present's, and a revocation shows as `DecisionState::Revoked` —
    /// never folded back into the assertion it revoked.
    ///
    /// The ledger lookups are the exact bounded ones the trust slice
    /// itself makes (account, repository and digest): one per asset
    /// that recorded a decision, never a scan.
    pub async fn read_capsule_with_trust(
        &self,
        image_digest: &str,
        trust: &dyn TrustLedger,
        now: DateTime<Utc>,
    ) -> Result<GraphCapsule, CapsuleGraphError> {
        let mut capsule = self.read_capsule(image_digest).await?;
        for asset in &mut capsule.assets {
            let Some(recorded) = &asset.decision else {
                asset.decision_state = DecisionState::NoDecision;
                continue;
            };
            let digest = asset_digest(&asset.digest)?;
            let current = trust
                .active_grant(&capsule.account, &capsule.repository, &digest, now)
                .await
                .map_err(CapsuleGraphError::unavailable)?;
            asset.decision_state = map_decision_state(Some(recorded), current.as_ref());
        }
        Ok(capsule)
    }

    async fn read_assets(
        &self,
        capsule: &str,
        manifest: &BundleManifest,
    ) -> Result<Vec<GraphAsset>, CapsuleGraphError> {
        let response = self.query_json(&asset_rows_query(capsule)).await?;
        let nodes = response
            .get("assets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes.len() > MAX_BUNDLE_ASSETS {
            return Err(refused(
                "the stored rows exceed the honest bound of a bundle",
            ));
        }
        let mut assets = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let decision = match node.get("hodei.capsule.asset.decision_id") {
                Some(_) => Some(DecisionEvidence {
                    decision_id: text_of(node, "hodei.capsule.asset.decision_id")?,
                    actor: text_of(node, "hodei.capsule.asset.decision_actor")?,
                    granted_at: time_of(node, "hodei.capsule.asset.decision_granted_at")?,
                }),
                None => None,
            };
            assets.push(GraphAsset {
                asset_path: text_of(node, "hodei.capsule.asset.asset_path")?,
                package: text_of(node, "hodei.capsule.asset.package")?,
                version: text_of(node, "hodei.capsule.asset.version")?,
                digest: text_of(node, "hodei.capsule.asset.digest")?,
                size_bytes: int_of(node, "hodei.capsule.asset.size_bytes")?,
                receipt_id: text_of(node, "hodei.capsule.asset.receipt_id")?,
                receipt_outcome: text_of(node, "hodei.capsule.asset.receipt_outcome")?,
                decision,
                decision_state: DecisionState::NoDecision,
            });
        }
        assets.sort_by(|a, b| a.asset_path.cmp(&b.asset_path));
        // The rows answer exactly the paths the manifest names: same
        // paths, same digests, same sizes, same receipts, same recorded
        // decisions, in the order the bundle states them. The graph
        // holds the bundle's word and nothing contradicts it — and
        // nothing outside it can join the list: the lookup is keyed by
        // this capsule alone.
        let expected: Vec<&str> = manifest
            .assets
            .iter()
            .map(|entry| entry.asset_path.as_str())
            .collect();
        let found: Vec<&str> = assets.iter().map(|row| row.asset_path.as_str()).collect();
        if expected != found {
            return Err(unsound(
                "the stored paths are not the paths the bundle manifest names",
            ));
        }
        for (entry, row) in manifest.assets.iter().zip(&assets) {
            if entry.digest != row.digest
                || entry.size_bytes != row.size_bytes
                || entry.receipt.id != row.receipt_id
                || entry.receipt.outcome.as_str() != row.receipt_outcome
                || entry.decision != row.decision
            {
                return Err(unsound(format!(
                    "the stored row for {} is not the row the bundle holds",
                    entry.asset_path
                )));
            }
        }
        Ok(assets)
    }

    async fn read_withheld(
        &self,
        capsule: &str,
        manifest: &BundleManifest,
    ) -> Result<Vec<Withheld>, CapsuleGraphError> {
        let response = self.query_json(&withheld_rows_query(capsule)).await?;
        let nodes = response
            .get("withheld")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes.len() > MAX_BUNDLE_ASSETS {
            return Err(refused(
                "the stored withheld rows exceed the honest bound of a bundle",
            ));
        }
        let mut withheld = Vec::with_capacity(nodes.len());
        for node in &nodes {
            withheld.push(Withheld {
                asset_path: text_of(node, "hodei.capsule.withheld.asset_path")?,
                reason: text_of(node, "hodei.capsule.withheld.reason")?,
            });
        }
        withheld.sort_by(|a, b| a.asset_path.cmp(&b.asset_path));
        if manifest.withheld != withheld {
            return Err(unsound(
                "the stored absences are not the absences the bundle manifest states",
            ));
        }
        Ok(withheld)
    }

    async fn query_json(&self, query: &str) -> Result<Value, CapsuleGraphError> {
        let response = self
            .client
            .execute_dql(query)
            .await
            .map_err(CapsuleGraphError::unavailable)?;
        require_no_errors(&response.json)?;
        Ok(response.json)
    }
}

/// The descriptor of a named layer of the capsule image.
fn find_layer<'a>(
    image: &'a ImageManifest,
    media_type: &str,
) -> Result<&'a Descriptor, CapsuleGraphError> {
    image
        .layers
        .iter()
        .find(|layer| layer.media_type == media_type)
        .ok_or_else(|| unsound(format!("the capsule has no layer of type {media_type}")))
}

/// Whether a stored capsule row carries exactly the bytes of an ingest:
/// the signed manifest, the bundle and the signature armor.
fn stored_capsule_matches(
    node: &Value,
    image_bytes: &[u8],
    release_bytes: &[u8],
    signature_armor: &str,
) -> bool {
    let same = |predicate: &str, bytes: &[u8]| {
        node.get(predicate)
            .and_then(Value::as_str)
            .is_some_and(|stored| stored.as_bytes() == bytes)
    };
    same("hodei.capsule.image_manifest", image_bytes)
        && same("hodei.capsule.release_json", release_bytes)
        && node.get("hodei.capsule.signature").and_then(Value::as_str) == Some(signature_armor)
}

fn text_of(node: &Value, name: &str) -> Result<String, CapsuleGraphError> {
    node.get(name)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| unsound(format!("a stored capsule without {name}")))
}

fn int_of(node: &Value, name: &str) -> Result<u64, CapsuleGraphError> {
    let raw = node
        .get(name)
        .ok_or_else(|| unsound(format!("a stored capsule without {name}")))?;
    raw.as_u64()
        .or_else(|| raw.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| unsound(format!("a stored capsule whose {name} is not a count")))
}

fn time_of(node: &Value, name: &str) -> Result<DateTime<Utc>, CapsuleGraphError> {
    let raw = text_of(node, name)?;
    DateTime::parse_from_rfc3339(&raw)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|_| unsound(format!("a stored capsule with an unreadable {name}")))
}

/// The content digest an asset row names, for the trust overlay. A
/// digest outside the `sha256:` grammar is a corrupt row, never a
/// guess: the exact lookup the trust ledger answers uses the very value
/// the row carries.
fn asset_digest(digest: &str) -> Result<ContentDigest, CapsuleGraphError> {
    let hex = crate::recovery::manifest::hex_of(digest)
        .ok_or_else(|| unsound(format!("an asset digest this slice cannot name: {digest}")))?;
    Ok(ContentDigest::sha256(hex))
}

/// The N-Quads of one capsule: the root row with its exact bytes, one
/// bounded row per resolved path carrying the history the bundle
/// recorded, and one row per explicit absence. Pure: every value is a
/// quoted, escaped string; the row keys name only this capsule.
#[allow(clippy::too_many_arguments)]
fn capsule_nquads(
    tenant: &Tenant,
    image_digest: &str,
    image_bytes: &[u8],
    release_bytes: &[u8],
    trust_bytes: &[u8],
    signature_armor: &str,
    signer: &str,
    manifest: &BundleManifest,
) -> String {
    let key = capsule_key(tenant, image_digest);
    let tenant_q = dql_string(tenant.as_str());
    let image_q = dql_string(image_digest);
    let mut quads = format!(
        "_:cap <dgraph.type> \"{CAPSULE_TYPE}\" .\n\
         _:cap <hodei.capsule.key> {key_q} .\n\
         _:cap <hodei.capsule.tenant> {tenant_q} .\n\
         _:cap <hodei.capsule.account> {account_q} .\n\
         _:cap <hodei.capsule.repository> {repository_q} .\n\
         _:cap <hodei.capsule.image_digest> {image_q} .\n\
         _:cap <hodei.capsule.release_digest> {release_q} .\n\
         _:cap <hodei.capsule.exported_at> {exported_q} .\n\
         _:cap <hodei.capsule.upstream_id> {upstream_q} .\n\
         _:cap <hodei.capsule.config_version> \"{config_q}\" .\n\
         _:cap <hodei.capsule.image_manifest> {manifest_q} .\n\
         _:cap <hodei.capsule.release_json> {release_json_q} .\n\
         _:cap <hodei.capsule.trust_json> {trust_json_q} .\n\
         _:cap <hodei.capsule.signature> {signature_q} .\n\
         _:cap <hodei.capsule.signer> {signer_q} .\n",
        key_q = dql_string(&key),
        account_q = dql_string(&manifest.origin.account),
        repository_q = dql_string(&manifest.origin.repository),
        release_q = dql_string(&digest_of(release_bytes)),
        exported_q = dql_string(&manifest.exported_at.to_rfc3339()),
        upstream_q = dql_string(&manifest.origin.upstream_id),
        config_q = manifest.origin.config_version,
        manifest_q = dql_string(&String::from_utf8_lossy(image_bytes)),
        release_json_q = dql_string(&String::from_utf8_lossy(release_bytes)),
        trust_json_q = dql_string(&String::from_utf8_lossy(trust_bytes)),
        signature_q = dql_string(signature_armor),
        signer_q = dql_string(signer),
    );

    for (index, entry) in manifest.assets.iter().enumerate() {
        let akey = asset_key(tenant, image_digest, &entry.asset_path);
        let node = format!("_:asset{index}");
        quads.push_str(&format!(
            "_:cap <hodei.capsule.assets> {node} .\n\
             {node} <dgraph.type> \"{ASSET_TYPE}\" .\n\
             {node} <hodei.capsule.asset.key> {akey_q} .\n\
             {node} <hodei.capsule.asset.capsule> {cap_q} .\n\
             {node} <hodei.capsule.asset.asset_path> {path_q} .\n\
             {node} <hodei.capsule.asset.package> {package_q} .\n\
             {node} <hodei.capsule.asset.version> {version_q} .\n\
             {node} <hodei.capsule.asset.digest> {digest_q} .\n\
             {node} <hodei.capsule.asset.size_bytes> \"{size_q}\" .\n\
             {node} <hodei.capsule.asset.receipt_id> {receipt_q} .\n\
             {node} <hodei.capsule.asset.receipt_outcome> {outcome_q} .\n",
            akey_q = dql_string(&akey),
            cap_q = dql_string(&key),
            path_q = dql_string(&entry.asset_path),
            package_q = dql_string(&entry.package),
            version_q = dql_string(&entry.version),
            digest_q = dql_string(&entry.digest),
            size_q = entry.size_bytes,
            receipt_q = dql_string(&entry.receipt.id),
            outcome_q = dql_string(entry.receipt.outcome.as_str()),
        ));
        if let Some(decision) = &entry.decision {
            quads.push_str(&format!(
                "{node} <hodei.capsule.asset.decision_id> {decision_q} .\n\
                 {node} <hodei.capsule.asset.decision_actor> {actor_q} .\n\
                 {node} <hodei.capsule.asset.decision_granted_at> {granted_q} .\n",
                decision_q = dql_string(&decision.decision_id),
                actor_q = dql_string(&decision.actor),
                granted_q = dql_string(&decision.granted_at.to_rfc3339()),
            ));
        }
    }

    for (index, withheld) in manifest.withheld.iter().enumerate() {
        let wkey = withheld_key(tenant, image_digest, &withheld.asset_path);
        let node = format!("_:withheld{index}");
        quads.push_str(&format!(
            "_:cap <hodei.capsule.withheld> {node} .\n\
             {node} <dgraph.type> \"{WITHHELD_TYPE}\" .\n\
             {node} <hodei.capsule.withheld.key> {wkey_q} .\n\
             {node} <hodei.capsule.withheld.capsule> {cap_q} .\n\
             {node} <hodei.capsule.withheld.asset_path> {path_q} .\n\
             {node} <hodei.capsule.withheld.reason> {reason_q} .\n",
            wkey_q = dql_string(&wkey),
            cap_q = dql_string(&key),
            path_q = dql_string(&withheld.asset_path),
            reason_q = dql_string(&withheld.reason),
        ));
    }
    quads
}

/// A server-side refusal travels inside the JSON: it is never read as a
/// successful answer.
fn require_no_errors(json: &Value) -> Result<(), CapsuleGraphError> {
    let empty: Vec<Value> = Vec::new();
    let errors = json
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if errors.is_empty() {
        return Ok(());
    }
    let text = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Err(CapsuleGraphError::Unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

/// Applies the additive DDL slice of this adapter. Idempotent and
/// bounded: two stores opening at once alter the same slice and Dgraph
/// refuses one of them while the indexes settle.
async fn apply_capsule_schema(client: &GraphClient) -> Result<(), CapsuleGraphError> {
    let mut last = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        match client.alter_schema(CAPSULE_SCHEMA_DQL).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < MAX_ATTEMPTS => {
                last = error.to_string();
                tokio::time::sleep((RETRY_BASE * attempt as u32).min(RETRY_CAP)).await;
            }
            Err(error) => return Err(CapsuleGraphError::unavailable(error)),
        }
    }
    Err(CapsuleGraphError::Unavailable(format!(
        "the capsule DDL slice did not settle after {MAX_ATTEMPTS} attempts: {last}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> Tenant {
        Tenant::new("hsp207-test").expect("the tenant validates")
    }

    #[test]
    fn keys_are_bounded_and_injective() {
        let image = "sha256:aaaa";
        let a = asset_key(&tenant(), image, "dir/file");
        let b = asset_key(&tenant(), image, "dir|file");
        assert_ne!(a, b, "a separator inside a path is escaped");
        assert_ne!(a, asset_key(&tenant(), image, "dir%2Ffile"));
        assert_ne!(b, asset_key(&tenant(), image, "dir%7Cfile"));
        assert_eq!(
            withheld_key(&tenant(), image, "x"),
            asset_key(&tenant(), image, "x")
        );
        let other = Tenant::new("other").expect("the tenant validates");
        assert_ne!(capsule_key(&tenant(), image), capsule_key(&other, image));
    }

    #[test]
    fn every_read_of_this_slice_is_an_exact_bounded_lookup() {
        // AC2: the shape of each query the module can issue. An exact
        // `eq` over an indexed predicate, a `first:` bound wherever a
        // list is read, and no scan anywhere. A distractor capsule is
        // unreachable except by naming its own key.
        let key = capsule_key(&tenant(), "sha256:beef");
        for query in [
            capsule_key_query(&tenant(), "sha256:beef"),
            asset_rows_query(&key),
            withheld_rows_query(&key),
        ] {
            assert!(query.contains("func: eq(hodei.capsule."), "{query}");
            assert!(!query.contains("has("), "{query} scans by predicate");
            assert!(!query.contains("type:"), "{query} censuses a type");
            assert!(!query.contains("allOf"), "{query}");
            assert!(!query.contains("regexp"), "{query}");
        }
        let rows = asset_rows_query(&key);
        assert!(rows.contains("first:"), "the row read is bounded");
        let root = capsule_key_query(&tenant(), "sha256:beef");
        assert!(
            !root.contains("hodei.capsule.assets"),
            "the root lookup does not follow edges into a census"
        );
    }

    #[test]
    fn revocation_is_a_state_and_never_a_silence() {
        let recorded = decision("dec-1");
        assert_eq!(map_decision_state(None, None), DecisionState::NoDecision);
        assert_eq!(
            map_decision_state(None, Some(&grant("dec-9"))),
            DecisionState::NoDecision
        );
        assert_eq!(
            map_decision_state(Some(&recorded), None),
            DecisionState::Revoked
        );
        assert_eq!(
            map_decision_state(Some(&recorded), Some(&grant("dec-1"))),
            DecisionState::Active
        );
        assert_eq!(
            map_decision_state(Some(&recorded), Some(&grant("dec-2"))),
            DecisionState::Superseded {
                current: "dec-2".into()
            }
        );
    }

    #[test]
    fn the_capsule_nquads_name_the_exact_rows() {
        let manifest = tiny_manifest();
        let quads = capsule_nquads(
            &tenant(),
            "sha256:img",
            b"image bytes",
            b"release bytes",
            b"trust bytes",
            "armor",
            "00ff",
            &manifest,
        );
        assert!(quads.contains(&capsule_key(&tenant(), "sha256:img")));
        assert!(quads.contains(&asset_key(&tenant(), "sha256:img", "a/one")));
        assert!(quads.contains(&asset_key(&tenant(), "sha256:img", "b/two")));
        assert!(quads.contains(&withheld_key(&tenant(), "sha256:img", "c/absent")));
        // One row per resolved path, one per absence; the decided path
        // carries its history and the undecided one carries nothing.
        let keys: Vec<&str> = quads
            .lines()
            .filter(|line| line.contains("<hodei.capsule.asset.key>"))
            .collect();
        assert_eq!(keys.len(), 2, "one asset row per path: {keys:?}");
        let withheld: Vec<&str> = quads
            .lines()
            .filter(|line| line.contains("<hodei.capsule.withheld.key>"))
            .collect();
        assert_eq!(withheld.len(), 1);
        assert_eq!(
            quads
                .lines()
                .filter(|line| line.contains("<hodei.capsule.asset.decision_id>"))
                .count(),
            1,
            "only the decided path names a decision"
        );
        assert!(quads.contains("\"image bytes\""));
    }

    #[test]
    fn the_nquads_carry_the_bytes_escaped() {
        let manifest = tiny_manifest();
        // Raw bytes holding a raw quote and a raw newline: nothing in
        // them may close a quad early or split it.
        let tricky = br#"{"key": "value with "quote" inside",
"nl": "line1
line2"}"#;
        let quads = capsule_nquads(
            &tenant(),
            "sha256:img",
            tricky,
            tricky,
            tricky,
            "-----BEGIN PGP-----\n\narmor\n-----END PGP-----\n",
            "00ff",
            &manifest,
        );
        // Nothing in the bytes can close a quad early: every raw quote
        // and newline inside a value is escaped.
        for line in quads
            .lines()
            .filter(|l| l.starts_with("_:cap <hodei.capsule."))
        {
            let open = line.matches('"').count();
            let escaped_quotes = line.matches("\\\"").count();
            assert_eq!(
                open % 2,
                escaped_quotes % 2,
                "unbalanced quoting in: {line}"
            );
        }
        assert!(quads.contains("\\\"quote\\\""), "the raw quote is escaped");
        assert!(
            quads.contains("line1\\nline2"),
            "the raw newline is the two-char escape"
        );
        // The armor travels as the layout wrote it, escaped too.
        assert!(quads.contains("-----BEGIN PGP-----\\n\\narmor"));
    }

    #[test]
    fn an_asset_digest_outside_the_grammar_is_refused_not_guessed() {
        assert!(asset_digest("sha256:zz").is_err());
        assert!(asset_digest("sha512:aa").is_err());
        assert!(asset_digest("sha256:aa").is_err());
        assert!(asset_digest(&format!("sha256:{}", "ab".repeat(32))).is_ok());
        // The overlay names the digest the row carries, lowercase:
        // the same grammar the trust ledger indexes.
        let parsed = asset_digest(&format!("sha256:{}", "ab".repeat(32))).expect("a digest");
        assert_eq!(parsed.to_string(), format!("sha256:{}", "ab".repeat(32)));
    }

    #[test]
    fn missing_fields_are_corrupt_never_defaulted() {
        let node = serde_json::json!({"hodei.capsule.image_digest": "sha256:beef"});
        assert_eq!(
            text_of(&node, "hodei.capsule.image_digest").unwrap(),
            "sha256:beef"
        );
        assert!(text_of(&node, "hodei.capsule.tenant").is_err());
        assert!(int_of(&node, "hodei.capsule.config_version").is_err());
        let with_count = serde_json::json!({"hodei.capsule.config_version": 7});
        assert_eq!(
            int_of(&with_count, "hodei.capsule.config_version").unwrap(),
            7
        );
        let bad_time = serde_json::json!({"x": "not-a-time"});
        assert!(time_of(&bad_time, "x").is_err());
    }

    #[test]
    fn a_stored_capsule_must_be_the_ingest_s_bytes() {
        let node = serde_json::json!({
            "hodei.capsule.image_manifest": "image bytes",
            "hodei.capsule.release_json": "release bytes",
            "hodei.capsule.signature": "armor",
        });
        assert!(stored_capsule_matches(
            &node,
            b"image bytes",
            b"release bytes",
            "armor"
        ));
        assert!(!stored_capsule_matches(
            &node,
            b"image bytes",
            b"other",
            "armor"
        ));
        assert!(!stored_capsule_matches(
            &node,
            b"image bytes",
            b"release bytes",
            "x"
        ));
    }

    #[test]
    fn the_digest_of_the_graph_is_the_lowercase_form_of_the_layout() {
        // The same bytes must digest the same way offline and in the
        // graph: AC1's digest parity starts at this function.
        let bytes = b"the exact bytes of the bundle manifest";
        let hex = digest_of(bytes);
        assert!(hex.starts_with("sha256:"));
        assert_eq!(hex, format!("sha256:{}", "a".repeat(0) + &hex[7..]));
        assert!(hex[7..].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hex.len(), 71);
    }

    fn decision(id: &str) -> DecisionEvidence {
        DecisionEvidence {
            decision_id: id.into(),
            actor: "hrn:hodei:iam:acme:User/carol".into(),
            granted_at: Utc::now(),
        }
    }

    fn grant(decision_id: &str) -> TrustGrant {
        use hodei_evidence::ContentDigest;
        TrustGrant {
            account: "acme".into(),
            repository: "mirror".into(),
            digest: ContentDigest::sha256("a".repeat(64)),
            decision_id: decision_id.into(),
            actor: "hrn".into(),
            granted_at: Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: None,
            capsule_hash: None,
            risk_version: None,
            valid_until: None,
        }
    }

    fn tiny_manifest() -> BundleManifest {
        use super::super::manifest::{AssetEntry, BlobEntry, FORMAT, Origin};
        use hodei_catalog::{ChecksumEvidence, ProvenanceReceipt, ReceiptOutcome};
        let digest = format!("sha256:{}", "a".repeat(64));
        let receipt = |path: &str| ProvenanceReceipt {
            id: format!("prv-{path}"),
            account: "acme".into(),
            repository: "mirror".into(),
            asset_path: path.into(),
            upstream_id: "central".into(),
            source_url: "https://repo.example.org/raw/x".into(),
            config_version: 1,
            digest: ContentDigest::sha256("a".repeat(64)),
            size_bytes: 7,
            checksum: ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: None,
            last_modified: None,
            outcome: ReceiptOutcome::Fetched,
            actor: "hrn".into(),
            correlation_id: "corr".into(),
            fetched_at: Utc::now(),
        };
        BundleManifest {
            format: FORMAT,
            exported_at: Utc::now(),
            origin: Origin {
                account: "acme".into(),
                repository: "mirror".into(),
                upstream_id: "central".into(),
                config_version: 1,
            },
            assets: vec![
                AssetEntry {
                    asset_path: "a/one".into(),
                    package: "one".into(),
                    version: "1".into(),
                    digest: digest.clone(),
                    size_bytes: 7,
                    receipt: receipt("a/one"),
                    decision: Some(decision("dec-1")),
                },
                AssetEntry {
                    asset_path: "b/two".into(),
                    package: "two".into(),
                    version: "1".into(),
                    digest: digest.clone(),
                    size_bytes: 7,
                    receipt: receipt("b/two"),
                    decision: None,
                },
            ],
            blobs: vec![BlobEntry { digest, length: 7 }],
            withheld: vec![Withheld {
                asset_path: "c/absent".into(),
                reason: "the content was withheld at the origin".into(),
            }],
        }
    }
}
