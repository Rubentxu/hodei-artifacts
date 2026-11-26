//! Projection rebuild and semantic reconciliation for the cutover (HSP-211).
//!
//! The import in [`super::import`] answers *which rows exist*: the canonical
//! identity rules and the authorities fingerprint is the oracle. This module
//! answers the cutover's question: **origin and destination are equivalent per
//! account**, not "by count". Two states with the same number of rows may
//! be equivalent in nothing — a crossed tag, a grant with no evidence in the
//! CAS, a ledger head that lies — and a cutover authorized by isolated
//! counts is exactly the false green that doc-20 forbids.
//!
//! Rules, from most to least important:
//!
//! 1. **Equivalence is aggregated by logical key.** Each projection
//!    ([`SemanticProjection`]) is computed per account, current grant, tag,
//!    artifact path, referenced digest and ledger cursor. No
//!    comparison uses global counts: a shift between accounts is a
//!    divergence, not a compensation.
//! 2. **Missing and corruption never escalate to verified.** [`CutoverToken`]
//!    is only produced by [`gate_cutover`] when every projection matches and every
//!    referenced digest is in the CAS. There is no public constructor, no loose
//!    boolean: the state is called `Blocked` or `Verified`, and `Blocked`
//!    names the guilty account or row.
//! 3. **There is only one authorities fingerprint.** The oracle is
//!    [`super::import::DestinationState::fingerprint`]; this module
//!    reuses it for global equivalence and adds the semantics on top,
//!    without inventing a second canonical signature.
//! 4. **The rebuild is idempotent.** [`prepare`] is a pure function over
//!    the row map: rebuilding the same state twice produces the
//!    same semantic version and the same gate result (AC2).
//! 5. **The head does not lie.** If the maximum `event_ledger` position of a
//!    stream does not match its `event_ledger_head`, the state is corrupt
//!    and the rebuild aborts naming the stream: a divergent cursor prevents
//!    the cutover, it is never "normalized".

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::RecoveryError;
use super::import::{
    DIGEST_FIELDS, DestinationState, DigestReconciler, ImportFailure, ImportManifest, Row, RowSink,
    canonical_digest, canonicalize_id, read_batch, read_import_manifest,
};

/// Tables whose rows hang from an account. `iam_account` declares the account
/// by its `handle`; the rest, by the `account` field.
pub const ACCOUNT_TABLES: &[&str] = &[
    "catalog_repository",
    "catalog_version",
    "catalog_asset",
    "catalog_reference",
    "catalog_tag",
];

/// Tables that define an artifact path within an account and
/// repository.
pub const PATH_TABLES: &[&str] = &["catalog_asset", "catalog_reference"];

/// The table of current grants.
pub const GRANT_TABLE: &str = "trust_grant";

/// The table of tags.
pub const TAG_TABLE: &str = "catalog_tag";

/// The table of ledger heads (cursors).
pub const HEAD_TABLE: &str = "event_ledger_head";

/// The table of ledger events.
pub const LEDGER_TABLE: &str = "event_ledger";

/// The rebuild fails: the state is not projectable as it is.
#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    /// A row required for the projection does not carry its key field, or
    /// carries it in another shape. Corruption: fail-closed, it is not ignored.
    #[error("the row '{identity}' is not projectable: {detail}")]
    Corrupt {
        /// The guilty canonical identity.
        identity: String,
        /// Why it does not match.
        detail: String,
    },
    /// The ledger head does not back the highest position of its events.
    #[error("the ledger head of stream '{stream}' says {head}, its events reach {events}")]
    CursorMismatch {
        /// The guilty stream.
        stream: String,
        /// What the head says.
        head: u64,
        /// What the events reach.
        events: u64,
    },
    /// The origin bundle cannot be read or is not intact.
    #[error("the origin package failed: {0}")]
    Package(#[from] RecoveryError),
    /// The origin cannot be canonicalized (invalid bundle rows).
    #[error("the origin package is invalid: {0}")]
    Origin(#[from] ImportFailure),
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{byte:02x}"));
    }
    out
}

fn table_of(identity: &str) -> &str {
    identity.split(':').next().unwrap_or(identity)
}

fn str_field(row: &Row, identity: &str, field: &str) -> Result<String, ReconcileError> {
    let value = row.get(field).ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the row carries no '{field}'"),
    })?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ReconcileError::Corrupt {
            identity: identity.to_string(),
            detail: format!("the '{field}' is not a string"),
        })
}

fn num_field(row: &Row, identity: &str, field: &str) -> Result<u64, ReconcileError> {
    let value = row.get(field).ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the row carries no '{field}'"),
    })?;
    value.as_u64().ok_or_else(|| ReconcileError::Corrupt {
        identity: identity.to_string(),
        detail: format!("the '{field}' is not a non-negative integer"),
    })
}

/// The semantic projection of the state: each aggregate by **its** logical
/// key, never a global census. It is the unit with which origin and destination
/// are compared; its [`Self::version`] is the signature of the rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticProjection {
    /// Account -> digest over the full bundle of its rows
    /// (`table:identity = digest` sorted). Covers the account's repositories,
    /// versions, assets, references and tags.
    pub accounts: BTreeMap<String, String>,
    /// Current grant (canonical identity) -> digest of its content.
    pub grants: BTreeMap<String, String>,
    /// Tag (canonical identity) -> digest of its content.
    pub tags: BTreeMap<String, String>,
    /// Path `account|repository|path` -> digest of the row that occupies it.
    pub paths: BTreeMap<String, String>,
    /// Referenced content digest -> identities that reference it,
    /// joined by commas in order. It is the map that is checked against the CAS.
    pub digests: BTreeMap<String, String>,
    /// Ledger stream -> position declared by its head.
    pub cursors: BTreeMap<String, u64>,
}

impl SemanticProjection {
    /// The semantic version of the rebuilt state: a SHA-256 over the
    /// six aggregates in key order. Rebuilding the same state twice
    /// gives the same version (AC2); a divergence in any key
    /// changes it.
    pub fn version(&self) -> String {
        let mut hasher = Sha256::new();
        for (account, digest) in &self.accounts {
            hasher.update(format!("A {account} {digest}\n").as_bytes());
        }
        for (grant, digest) in &self.grants {
            hasher.update(format!("G {grant} {digest}\n").as_bytes());
        }
        for (tag, digest) in &self.tags {
            hasher.update(format!("T {tag} {digest}\n").as_bytes());
        }
        for (path, digest) in &self.paths {
            hasher.update(format!("P {path} {digest}\n").as_bytes());
        }
        for (digest, refs) in &self.digests {
            hasher.update(format!("D {digest} {refs}\n").as_bytes());
        }
        for (stream, position) in &self.cursors {
            hasher.update(format!("C {stream} {position}\n").as_bytes());
        }
        sha256_hex(&hasher.finalize())
    }
}

/// A state ready to compare: the canonical rows, its semantic
/// projection and the authorities fingerprint of the HSP-210 oracle.
#[derive(Debug, Clone)]
pub struct ReconcileSubject {
    /// The rows by canonical identity.
    pub rows: BTreeMap<String, Row>,
    /// The semantic projection rebuilt from those rows.
    pub projection: SemanticProjection,
    /// The authorities fingerprint (the only oracle, that of
    /// [`DestinationState::fingerprint`]).
    pub fingerprint: String,
}

impl ReconcileSubject {
    /// The semantic version of the state.
    pub fn version(&self) -> String {
        self.projection.version()
    }
}

/// The pure rebuild: from `table:identity -> row` to the semantic projection
/// and the authorities fingerprint. It is a single deterministic pass;
/// applying it twice over the same map preserves the version and the results.
pub fn prepare(rows: BTreeMap<String, Row>) -> Result<ReconcileSubject, ReconcileError> {
    let mut projection = SemanticProjection {
        accounts: BTreeMap::new(),
        grants: BTreeMap::new(),
        tags: BTreeMap::new(),
        paths: BTreeMap::new(),
        digests: BTreeMap::new(),
        cursors: BTreeMap::new(),
    };

    // Bundle per account: iam_account declares the account by its handle; the
    // catalog tables hang from the `account` field.
    let mut bundles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut heads: BTreeMap<String, u64> = BTreeMap::new();
    let mut ledger_max: BTreeMap<String, u64> = BTreeMap::new();

    for (identity, row) in &rows {
        let table = table_of(identity);
        let digest = canonical_digest(row);

        let account = match table {
            "iam_account" => Some(str_field(row, identity, "handle")?),
            t if ACCOUNT_TABLES.contains(&t) => Some(str_field(row, identity, "account")?),
            _ => None,
        };
        if let Some(account) = account {
            bundles
                .entry(account)
                .or_default()
                .push(format!("{identity}={digest}"));
        }

        if table == GRANT_TABLE {
            projection.grants.insert(identity.clone(), digest.clone());
        }
        if table == TAG_TABLE {
            projection.tags.insert(identity.clone(), digest.clone());
        }
        if PATH_TABLES.contains(&table) {
            let account = str_field(row, identity, "account")?;
            let repository = str_field(row, identity, "repository")?;
            let path = str_field(row, identity, "asset_path")?;
            projection
                .paths
                .insert(format!("{account}|{repository}|{path}"), digest.clone());
        }

        for (name, fields) in DIGEST_FIELDS {
            if *name != table {
                continue;
            }
            for field in *fields {
                if let Some(value) = row.get(*field).and_then(serde_json::Value::as_str)
                    && !value.is_empty()
                {
                    let refs = projection.digests.entry(value.to_string()).or_default();
                    if refs.is_empty() {
                        refs.push_str(identity);
                    } else {
                        let _ = std::fmt::Write::write_fmt(refs, format_args!(",{identity}"));
                    }
                }
            }
        }

        if table == HEAD_TABLE {
            let stream = str_field(row, identity, "stream")?;
            let last = num_field(row, identity, "last_position")?;
            heads.insert(stream, last);
        }
        if table == LEDGER_TABLE {
            let stream = str_field(row, identity, "stream")?;
            let position = num_field(row, identity, "position")?;
            let entry = ledger_max.entry(stream).or_default();
            *entry = (*entry).max(position);
        }
    }

    for (account, mut facts) in bundles {
        facts.sort();
        let joined = facts.join("\n");
        projection
            .accounts
            .insert(account, sha256_hex(joined.as_bytes()));
    }

    // The head does not lie: its position has to be exactly the highest
    // of its events. A stream with no events cannot declare a head.
    for (stream, head) in &heads {
        let events = ledger_max.get(stream).copied().unwrap_or(0);
        if *head != events {
            return Err(ReconcileError::CursorMismatch {
                stream: stream.clone(),
                head: *head,
                events,
            });
        }
    }
    projection.cursors = heads;

    let mut tables: BTreeMap<String, u64> = BTreeMap::new();
    let mut row_digests = BTreeMap::new();
    for (identity, row) in &rows {
        *tables.entry(table_of(identity).to_string()).or_default() += 1;
        row_digests.insert(identity.clone(), canonical_digest(row));
    }
    let fingerprint = DestinationState {
        tables,
        rows: row_digests,
    }
    .fingerprint();

    Ok(ReconcileSubject {
        rows,
        projection,
        fingerprint,
    })
}

/// The state of an origin↔destination comparison. Explicit by construction:
/// there is no loose "ok" boolean that missing and corruption could escalate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Divergence {
    /// A logical key of the origin does not exist at the destination. **Missing**:
    /// it prevents the cutover; it is never conformed into verified.
    #[error("missing on the destination: {what}")]
    Missing {
        /// Which aggregate and which key.
        what: String,
    },
    /// A logical key of the destination does not exist at the origin.
    #[error("extra on the destination: {what}")]
    Extra {
        /// Which aggregate and which key.
        what: String,
    },
    /// The same logical key, different canonical content.
    #[error("{what} differs: origin {origin}, destination {destination}")]
    Content {
        /// Which aggregate and which key.
        what: String,
        /// Digest of the origin.
        origin: String,
        /// Digest of the destination.
        destination: String,
    },
    /// The cursor position is not equivalent.
    #[error(
        "the ledger cursor of stream '{stream}' is at origin {origin}, destination {destination}"
    )]
    Cursor {
        /// The guilty stream.
        stream: String,
        /// The origin position.
        origin: u64,
        /// The destination position.
        destination: u64,
    },
    /// The CAS does not have the digest that a row references. **Missing/corrupt
    /// content**: fail-closed, naming the referent.
    #[error("the CAS lacks the digest {digest} referenced by {referenced_by}")]
    MissingDigest {
        /// The missing digest.
        digest: String,
        /// The first row (origin or destination) that references it.
        referenced_by: String,
    },
    /// The authorities fingerprint is not equivalent even though the projections
    /// match: there are rows outside the aggregates (decisions, outbox,
    /// ledger) that diverge.
    #[error("the authority fingerprint differs: origin {origin}, destination {destination}")]
    Authority {
        /// Fingerprint of the origin.
        origin: String,
        /// Fingerprint of the destination.
        destination: String,
    },
}

/// The result of the gate. [`CutoverDecision::Verified`] carries the single
/// token that authorizes the cut; [`CutoverDecision::Blocked`] lists each
/// divergence with its guilty party named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CutoverDecision {
    /// Origin and destination are equivalent per account, supports, tags, digests,
    /// paths and cursors, and the CAS backs every referenced digest.
    Verified(CutoverToken),
    /// Something diverges. The cutover is prevented; no warnings.
    Blocked(Vec<Divergence>),
}

impl CutoverDecision {
    /// The token if and only if the decision is verified.
    pub fn token(&self) -> Option<&CutoverToken> {
        match self {
            Self::Verified(token) => Some(token),
            Self::Blocked(_) => None,
        }
    }
}

/// The proof that the gate passed. **It cannot be forged from outside**:
/// its constructor is private and its fields are private. A `Verified`
/// without a token does not exist; a missing or corrupt never reaches this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutoverToken {
    origin_fingerprint: String,
    destination_fingerprint: String,
    version: String,
}

impl CutoverToken {
    /// The authorities fingerprint that was verified at the origin.
    pub fn origin_fingerprint(&self) -> &str {
        &self.origin_fingerprint
    }

    /// The authorities fingerprint that was verified at the destination.
    pub fn destination_fingerprint(&self) -> &str {
        &self.destination_fingerprint
    }

    /// The semantic version common to both states.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// The destination rebuild from a sink of the import contract: same
/// `table:identity -> row` map that the import sees, same projection.
pub async fn prepare_from_sink(sink: &dyn RowSink) -> Result<ReconcileSubject, ReconcileError> {
    let summary = sink.state_summary().await?;
    let mut rows = BTreeMap::new();
    for identity in summary.rows.keys() {
        let row = sink
            .lookup(identity)
            .await?
            .ok_or_else(|| ReconcileError::Corrupt {
                identity: identity.clone(),
                detail: "the state summary lists a row the sink cannot return".to_string(),
            })?;
        rows.insert(identity.clone(), row);
    }
    prepare(rows)
}

/// The rows of the origin bundle, canonicalized with the same identity that
/// the import fixes. It reuses the manifest-first verification of HSP-210:
/// a tampered or out-of-bounds batch aborts before projecting anything.
pub fn package_rows(package: &Path) -> Result<BTreeMap<String, Row>, ReconcileError> {
    let manifest: ImportManifest =
        read_import_manifest(package).map_err(ReconcileError::Package)?;
    let mut rows = BTreeMap::new();
    for table in &manifest.tables {
        for batch in &table.batches {
            for row in read_batch(package, batch).map_err(ReconcileError::Package)? {
                let identity =
                    canonicalize_id(&table.table, &row).map_err(ReconcileError::Origin)?;
                if rows.insert(identity.clone(), row).is_some() {
                    return Err(ReconcileError::Origin(ImportFailure::DuplicateRow {
                        table: table.table.clone(),
                        key: identity,
                    }));
                }
            }
        }
    }
    Ok(rows)
}

/// The cutover gate. It rebuilds the comparison over the already prepared
/// projections of origin and destination and checks each referenced digest
/// against the CAS. It returns `Blocked` with the full list of guilty parties;
/// `Verified` requires that there be none (AC1 fail-closed) and that the two
/// projections have the same version.
pub async fn gate_cutover(
    origin: &ReconcileSubject,
    destination: &ReconcileSubject,
    cas: &dyn DigestReconciler,
) -> Result<CutoverDecision, RecoveryError> {
    let mut divergences = Vec::new();
    compare_map(
        "account",
        &origin.projection.accounts,
        &destination.projection.accounts,
        &mut divergences,
    );
    compare_map(
        "grant",
        &origin.projection.grants,
        &destination.projection.grants,
        &mut divergences,
    );
    compare_map(
        "tag",
        &origin.projection.tags,
        &destination.projection.tags,
        &mut divergences,
    );
    compare_map(
        "path",
        &origin.projection.paths,
        &destination.projection.paths,
        &mut divergences,
    );
    compare_map(
        "digest-reference",
        &origin.projection.digests,
        &destination.projection.digests,
        &mut divergences,
    );
    for stream in origin
        .projection
        .cursors
        .keys()
        .chain(destination.projection.cursors.keys())
    {
        match (
            origin.projection.cursors.get(stream),
            destination.projection.cursors.get(stream),
        ) {
            (Some(from), Some(to)) if from != to => divergences.push(Divergence::Cursor {
                stream: stream.clone(),
                origin: *from,
                destination: *to,
            }),
            (Some(_), None) => divergences.push(Divergence::Missing {
                what: format!("cursor stream '{stream}'"),
            }),
            (None, Some(_)) => divergences.push(Divergence::Extra {
                what: format!("cursor stream '{stream}'"),
            }),
            _ => {}
        }
    }

    if origin.fingerprint != destination.fingerprint {
        divergences.push(Divergence::Authority {
            origin: origin.fingerprint.clone(),
            destination: destination.fingerprint.clone(),
        });
    }

    // CAS: every digest that either side references has to be there. The
    // "first referencing" travels in the error to name the guilty party.
    let mut digests: BTreeMap<&String, &String> = BTreeMap::new();
    for (digest, refs) in origin.projection.digests.iter() {
        digests.insert(digest, refs);
    }
    for (digest, refs) in destination.projection.digests.iter() {
        digests.entry(digest).or_insert(refs);
    }
    for (digest, referenced_by) in digests {
        if !cas.has(digest).await? {
            divergences.push(Divergence::MissingDigest {
                digest: digest.clone(),
                referenced_by: referenced_by.clone(),
            });
        }
    }

    if !divergences.is_empty() {
        return Ok(CutoverDecision::Blocked(divergences));
    }
    let version = origin.version();
    if version != destination.version() {
        return Ok(CutoverDecision::Blocked(vec![Divergence::Content {
            what: "projection version".to_string(),
            origin: version,
            destination: destination.version(),
        }]));
    }
    Ok(CutoverDecision::Verified(CutoverToken {
        origin_fingerprint: origin.fingerprint.clone(),
        destination_fingerprint: destination.fingerprint.clone(),
        version,
    }))
}

fn compare_map(
    what: &str,
    origin: &BTreeMap<String, String>,
    destination: &BTreeMap<String, String>,
    divergences: &mut Vec<Divergence>,
) {
    let keys: std::collections::BTreeSet<&String> =
        origin.keys().chain(destination.keys()).collect();
    for key in keys {
        match (origin.get(key), destination.get(key)) {
            (Some(from), Some(to)) if from != to => divergences.push(Divergence::Content {
                what: format!("{what} '{key}'"),
                origin: from.clone(),
                destination: to.clone(),
            }),
            (Some(_), None) => divergences.push(Divergence::Missing {
                what: format!("{what} '{key}'"),
            }),
            (None, Some(_)) => divergences.push(Divergence::Extra {
                what: format!("{what} '{key}'"),
            }),
            _ => {}
        }
    }
}
