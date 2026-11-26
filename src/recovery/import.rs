//! Idempotent import of persistent authorities (HSP-210 / HGS-28).
//!
//! The backup of [`super::backup`] is a mirror of the engine: it only goes back to a
//! **clean** destination and carries the metadata as opaque SurrealQL. This
//! module is the other half of the recovery contract: the portable
//! semantic bundle produced by the export tool (HSP-179,
//! doc-20) —canonical JSON rows per table, in dependency order, with a
//! canonical `table:key` identity— is imported **against an authority that
//! already has data**, and doing so twice cannot duplicate anything.
//!
//! Rules, from most to least important:
//!
//! 1. **The canonical identity decides, the physical UID never.** The key of
//!    a row is its canonicalized `id` (`table:key`, the same notion that
//!    the export fixes when writing its batches): it is looked up by it and
//!    written with it. The destination's internal UIDs are neither copied nor
//!    reassigned: a row that is already there, with the same content, is not touched;
//!    a new row is created **with the origin's identity**.
//! 2. **Manifest-first, fail-closed.** Before writing a single row the
//!    whole bundle is checked: format, self-digest, batch hashes
//!    against file, counts, order, batch bounds, repeated
//!    identities and content digests against the CAS. A bundle that does not
//!    match aborts naming the guilty party; "what there was" is never imported.
//! 3. **The manifest order is the application order.** Tables
//!    travel in dependency order (repositories before versions and
//!    assets, the ledger after what it references); batches travel
//!    sorted by identity. The import keeps that order and does not
//!    reorder it.
//! 4. **A divergent row is an operator decision, not a technical
//!    tie.** The same identity with different content is a typed `Conflict`
//!    and aborts the run. Only an explicitly authorized rewrite
//!    (`overwrite`) repairs it, and even then the canonical identity
//!    is kept.
//! 5. **Every run leaves a resumable record.** Progress is written
//!    atomically after each batch; a cut midway is resumed by verifying
//!    each recorded batch against its hash **and against the destination** before
//!    skipping it. Resuming after the cut and repeating the whole import
//!    converge to the same state.
//!
//! The application lives behind [`RowSink`]: the domain decides (identity,
//! order, conflict, progress) and the adapter touches the store. The adapter
//! over the current Dgraph profile is a seam recorded as a gap in the
//! evidence (`target/uat/hsp210/evidencia.md`); the core here is
//! engine-agnostic and is tested on a real in-memory engine.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RecoveryError;

/// The version of the importable bundle format.
///
/// It goes up when an old bundle can no longer be read. A bundle with a
/// format that this binary does not understand is **rejected** naming both
/// numbers, just like the backup of [`super::manifest`].
pub const IMPORT_FORMAT: u32 = 1;

/// The manifest inside the bundle directory.
pub const IMPORT_MANIFEST_FILE: &str = "manifest.json";

/// The batches directory inside the bundle.
pub const IMPORT_BATCHES_DIR: &str = "batches";

/// The canonical name of the progress marker of a half-finished run.
///
/// It lives next to the authority, not next to the bundle: the bundle is
/// immutable, progress is application state. Its presence
/// means "unfinished run"; it is deleted on finishing.
pub const IMPORT_PROGRESS_FILE: &str = ".import-progress.json";

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Where the bundle came from. It is metadata of the origin: the import records it
/// in the report but never writes it to the destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSource {
    /// The origin's engine.
    pub engine: String,
    /// The version of the engine contract.
    pub engine_version: String,
    /// The origin's backend, as declared by whoever operates.
    pub backend: String,
    /// The origin's namespace.
    pub namespace: String,
    /// The origin's database.
    pub database: String,
}

/// The boundary of the source at the moment of export.
///
/// The import keeps it and reports it: it is part of the signature of **what** was
/// imported. Two imports of the same bundle have the same boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFrontier {
    /// Row count per table.
    pub tables: BTreeMap<String, u64>,
    /// Rows of the evidence ledger.
    pub ledger_rows: u64,
    /// The highest ledger position, when it is not empty.
    pub ledger_max_position: Option<u64>,
    /// Rows of the ledger heads table.
    pub head_rows: u64,
}

/// A batch of the bundle, and the hash that fixes its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBatch {
    /// The file, relative to the bundle (`batches/<table>-<i>.json`).
    pub file: String,
    /// The SHA-256 of the file, in lowercase hexadecimal.
    pub sha256: String,
    /// Rows the batch carries.
    pub rows: u64,
    /// The identity of the first row of the batch.
    pub first_id: String,
    /// The identity of the last row of the batch.
    pub last_id: String,
}

/// The batches of a table, in application order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTable {
    /// The table.
    pub table: String,
    /// Total rows of the table at the source.
    pub rows: u64,
    /// The batches, in order.
    pub batches: Vec<ImportBatch>,
}

/// The blob policy declared by the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobPolicy {
    /// Why the bytes did not travel.
    pub policy: String,
    /// Whether the bytes traveled: never.
    pub copied: bool,
}

/// What the bundle says about itself.
///
/// Read mirror of format 1 written by the export tool
/// (HSP-179): the serde keys match, so that a real export is
/// imported without translation. Tables travel **in dependency order**;
/// the order of the vector is a contract, not a suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportManifest {
    /// The format version.
    pub format: u32,
    /// The version of the tool that produced it.
    pub tool_version: String,
    /// The origin of the rows.
    pub source: ExportSource,
    /// The boundary of the source.
    pub frontier: ExportFrontier,
    /// The tables, in application order.
    pub tables: Vec<ImportTable>,
    /// The blob policy: the digests traveled, the bytes did not.
    pub blobs: BlobPolicy,
    /// The SHA-256 of the whole content: the batch hashes concatenated
    /// in order, each followed by a line break.
    pub content_sha256: String,
}

impl ImportManifest {
    /// The self-digest recomputed from the batch hashes.
    pub fn content_digest(&self) -> String {
        let mut input = Vec::new();
        for table in &self.tables {
            for batch in &table.batches {
                input.extend_from_slice(batch.sha256.as_bytes());
                input.push(b'\n');
            }
        }
        sha256_hex(&input)
    }

    /// The batches of all tables, flattened in application order.
    pub fn all_batches(&self) -> Vec<(&str, &ImportBatch)> {
        self.tables
            .iter()
            .flat_map(|table| {
                table
                    .batches
                    .iter()
                    .map(move |batch| (table.table.as_str(), batch))
            })
            .collect()
    }

    /// Checks the bundle **before** touching anything: format, tables without
    /// repetition, non-empty batches, batch order by identity and coherence
    /// of counts and of the boundary. Each problem is typed.
    pub fn check(&self) -> Result<(), ImportFailure> {
        if self.format != IMPORT_FORMAT {
            return Err(ImportFailure::Format {
                found: self.format,
                expected: IMPORT_FORMAT,
            });
        }
        let mut names = BTreeSet::new();
        for table in &self.tables {
            if !names.insert(table.table.as_str()) {
                return Err(ImportFailure::Invalid(format!(
                    "the manifest repeats the table '{}'",
                    table.table
                )));
            }
            let mut counted = 0u64;
            let mut previous_last: Option<&str> = None;
            for batch in &table.batches {
                if batch.rows == 0 {
                    return Err(ImportFailure::Invalid(format!(
                        "the batch '{}' of '{}' is empty",
                        batch.file, table.table
                    )));
                }
                if let Some(previous) = previous_last
                    && previous > batch.first_id.as_str()
                {
                    return Err(ImportFailure::Order {
                        table: table.table.clone(),
                        previous: previous.to_string(),
                        found: batch.first_id.clone(),
                    });
                }
                previous_last = Some(&batch.last_id);
                counted += batch.rows;
            }
            if counted != table.rows {
                return Err(ImportFailure::CountMismatch {
                    table: table.table.clone(),
                    declared: table.rows,
                    batches: counted,
                });
            }
        }
        let frontier_total: u64 = self.frontier.tables.values().sum();
        let batch_total: u64 = self.tables.iter().map(|t| t.rows).sum();
        if frontier_total != batch_total {
            return Err(ImportFailure::FrontierMismatch {
                frontier: frontier_total,
                batches: batch_total,
            });
        }
        if self.content_sha256 != self.content_digest() {
            return Err(ImportFailure::ContentDigest {
                declared: self.content_sha256.clone(),
                computed: self.content_digest(),
            });
        }
        Ok(())
    }
}

/// Reads and checks the manifest of an export bundle.
pub fn read_import_manifest(directory: &Path) -> Result<ImportManifest, RecoveryError> {
    let text = std::fs::read(directory.join(IMPORT_MANIFEST_FILE)).map_err(|error| {
        RecoveryError::Unsound(format!(
            "{} cannot be read ({error}): the export is not finished or is not an export",
            IMPORT_MANIFEST_FILE
        ))
    })?;
    let manifest: ImportManifest = serde_json::from_slice(&text).map_err(|error| {
        RecoveryError::Unsound(format!("the export manifest is invalid: {error}"))
    })?;
    manifest
        .check()
        .map_err(|failure| RecoveryError::Unsound(failure.to_string()))?;
    Ok(manifest)
}

/// The row as it travels in a batch: a JSON object with its `id`.
pub type Row = serde_json::Map<String, serde_json::Value>;

/// The state of the destination that the core needs in order to decide. It is read by the
/// sink; the core does not touch the store.
#[async_trait]
pub trait RowSink: Send + Sync {
    /// Looks up by full canonical identity (`table:key`).
    async fn lookup(&self, identity: &str) -> Result<Option<Row>, RecoveryError>;

    /// Writes the row **with the given canonical identity** as key. The
    /// adapter does not choose another: keeping the key is the contract (AC1).
    async fn put(&self, identity: &str, row: &Row) -> Result<(), RecoveryError>;

    /// The canonical digest of a row: the key of equivalence.
    ///
    /// The default is the structural identity of the JSON (sorted keys,
    /// without the `id`, which is already the key). An adapter with its own
    /// representation of the origin can equate it here so that "same content"
    /// means the same at origin and destination.
    fn fingerprint_row(&self, row: &Row) -> String {
        canonical_digest(row)
    }

    /// The summary of the destination that the AC answers for: count per
    /// table and the global digest of the rows that exist.
    async fn state_summary(&self) -> Result<DestinationState, RecoveryError>;
}

/// The state of the destination, as the import sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationState {
    /// Rows per table.
    pub tables: BTreeMap<String, u64>,
    /// The canonical digest of each row, sorted as `table:id`.
    /// It is the invariant of the authorities fingerprint: two states with
    /// the same map have the same authorities, not "similar" ones.
    pub rows: BTreeMap<String, String>,
}

impl DestinationState {
    /// The authorities fingerprint: a SHA-256 over the canonical
    /// summary of the destination. It is what two imports of the same
    /// bundle must produce equal (AC1).
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        for (table, count) in &self.tables {
            hasher.update(format!("T {table} {count}\n").as_bytes());
        }
        for (key, digest) in &self.rows {
            hasher.update(format!("R {key} {digest}\n").as_bytes());
        }
        sha256_hex(&hasher.finalize())
    }
}

/// The canonical digest of a row: serialization with sorted keys
/// (serde_json's `Map` serializes in parser order; for the
/// contract it is reordered explicitly) and without the `id` field, which is the
/// key and is compared separately.
pub fn canonical_digest(row: &Row) -> String {
    let mut content = Row::new();
    for (key, value) in row {
        if key == "id" {
            continue;
        }
        content.insert(key.clone(), canonical_value(value));
    }
    // BTreeMap gives sorted keys: the digest does not depend on the order of
    // the keys in the origin file.
    let ordered: BTreeMap<&String, serde_json::Value> =
        content.iter().map(|(k, v)| (k, v.clone())).collect();
    let bytes = serde_json::to_vec(&ordered).unwrap_or_default();
    sha256_hex(&bytes)
}

/// Reorders nested objects so that the digest is canonical from top
/// to bottom: an origin that writes `{"b":1,"a":2}` and a destination that reads it
/// the other way around are the same authority.
fn canonical_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let ordered: BTreeMap<&String, serde_json::Value> = object
                .iter()
                .map(|(k, v)| (k, canonical_value(v)))
                .collect();
            serde_json::Value::Object(
                ordered
                    .into_iter()
                    .map(|(k, v)| (k.clone(), v))
                    .collect::<Row>(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_value).collect())
        }
        other => other.clone(),
    }
}

/// The mode of the run.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Maximum page size of writes per block. It bounds the work
    /// per transaction of the adapter; the bundle's batch rules.
    pub batch_rows: usize,
    /// Resume instead of starting: the progress recorded at the destination
    /// rules, and each already-applied batch is **verified against its hash and
    /// against the destination** before skipping it. A progress that does not match
    /// aborts.
    pub resume: bool,
    /// Rewrite divergent rows while keeping their canonical identity.
    /// Without this, a divergence aborts the run (AC2).
    pub overwrite: bool,
    /// The operator waived the reconciliation of digests against the CAS.
    /// Without a reconciler and without this flag the import aborts before
    /// writing: the contract is not fulfilled blindly.
    pub allow_missing_blobs: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            batch_rows: 200,
            resume: false,
            overwrite: false,
            allow_missing_blobs: false,
        }
    }
}

/// Fields of the rows that carry content digests, per table. The
/// list is deliberately short and explicit: the reconciler does not
/// scan values by guessing, it walks these keys.
pub const DIGEST_FIELDS: &[(&str, &[&str])] = &[
    ("catalog_asset", &["sha256"]),
    ("catalog_reference", &["sha256"]),
    ("provenance_receipt", &["digest"]),
    ("trust_grant", &["evidence"]),
    ("catalog_audit_outbox", &["payload_digest"]),
];

/// What an import run did, batch by batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchOutcome {
    /// The table.
    pub table: String,
    /// The batch number (1-based).
    pub index: usize,
    /// The batch file.
    pub file: String,
    /// New rows created with their canonical identity.
    pub created: u64,
    /// Rows that were already identical: skipped.
    pub skipped: u64,
    /// Divergent rows rewritten with authorization (keeping the id).
    pub overwritten: u64,
}

/// What an import run did.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// The authorities fingerprint **before** the run.
    pub fingerprint_before: String,
    /// The authorities fingerprint **after**.
    pub fingerprint_after: String,
    /// The boundary of the bundle, copied from the manifest.
    pub frontier: ExportFrontier,
    /// The self-digest of the bundle.
    pub content_sha256: String,
    /// The batches applied by this run (the resumed ones do not appear).
    pub batches: Vec<BatchOutcome>,
    /// Totals per category.
    pub created: u64,
    pub skipped: u64,
    pub overwritten: u64,
    /// Whether the run finished (it deleted the progress marker).
    pub finished: bool,
    /// When it started (informative; it does not enter any digest).
    pub started_at: DateTime<Utc>,
    /// How long it took.
    pub elapsed_ms: u64,
}

/// The state of a half-finished run, recorded at the destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportProgress {
    /// The version of the marker format.
    pub format: u32,
    /// The self-digest of the bundle being applied. A progress from
    /// another bundle aborts the resume: resuming against a different
    /// bundle is not resuming.
    pub content_sha256: String,
    /// The applied batches, in order.
    pub done: Vec<BatchOutcome>,
}

/// Why an import aborts. Each variant names the guilty party (AC2).
#[derive(Debug, thiserror::Error)]
pub enum ImportFailure {
    /// The bundle declares a format that this binary cannot read.
    #[error("the export uses format {found}; this build reads format {expected}")]
    Format {
        /// The format of the bundle.
        found: u32,
        /// The format that this binary writes.
        expected: u32,
    },
    /// The manifest's self-digest does not match its batches.
    #[error("the manifest content digest is {declared} but its batches hash to {computed}")]
    ContentDigest {
        /// What the manifest declares.
        declared: String,
        /// What its batches compute.
        computed: String,
    },
    /// The sum of rows of a table's batches is not the declared one.
    #[error("the table {table} declares {declared} rows but its batches carry {batches}")]
    CountMismatch {
        /// The table.
        table: String,
        /// What is declared.
        declared: u64,
        /// What the batches sum to.
        batches: u64,
    },
    /// The boundary and the batches do not count the same.
    #[error("the frontier counts {frontier} rows but the batches carry {batches}")]
    FrontierMismatch {
        /// What the boundary counts.
        frontier: u64,
        /// What the batches carry.
        batches: u64,
    },
    /// The batches do not come sorted by identity.
    #[error("the batches of {table} are out of order: '{previous}' then '{found}'")]
    Order {
        /// The table.
        table: String,
        /// The last id of the previous batch.
        previous: String,
        /// The first id that violates it.
        found: String,
    },
    /// A row of the batch cannot be canonicalized.
    #[error("the row '{id}' of {table} is invalid: {detail}")]
    RowInvalid {
        /// The table.
        table: String,
        /// The declared or inferred id.
        id: String,
        /// Why it is not valid.
        detail: String,
    },
    /// An identity outside the range of its batch's bounds.
    #[error("the row '{id}' of batch '{file}' is outside the batch range '{first}'..='{last}'")]
    RowOutOfBatch {
        /// The guilty identity.
        id: String,
        /// The batch file.
        file: String,
        /// The declared lower bound.
        first: String,
        /// The declared upper bound.
        last: String,
    },
    /// The same canonical identity appears twice in the bundle.
    #[error("the export repeats the canonical identity '{key}' of table {table}")]
    DuplicateRow {
        /// The table.
        table: String,
        /// The repeated identity.
        key: String,
    },
    /// A canonical identity already exists at the destination with other content
    /// and the rewrite is not authorized. Fail-closed.
    #[error(
        "the destination already has '{key}' in {table} with different content \
         (package digest {package_digest}, destination digest {destination_digest}); pass \
         overwrite to repair it keeping the canonical identity"
    )]
    Conflict {
        /// The table.
        table: String,
        /// The divergent identity.
        key: String,
        /// The canonical digest of the bundle's row.
        package_digest: String,
        /// The canonical digest of the destination's row.
        destination_digest: String,
    },
    /// The bundle references a content digest that the CAS does not have.
    #[error(
        "the CAS lacks {missing} referenced blob(s); first is {first} referenced by \
         {referenced_by}"
    )]
    MissingBlobs {
        /// How many are missing.
        missing: usize,
        /// The first one.
        first: String,
        /// Who references it.
        referenced_by: String,
    },
    /// The recorded progress does not belong to this bundle.
    #[error(
        "the destination holds progress for content {progress} and this package is {package}; \
         resume would apply the wrong batches"
    )]
    ProgressMismatch {
        /// The progress that is there.
        progress: String,
        /// The bundle that is brought.
        package: String,
    },
    /// The progress promises a batch that the bundle or the destination do not back.
    #[error("the progress for batch '{file}' of {table} is not backed: {detail}")]
    ProgressNotBacked {
        /// The table.
        table: String,
        /// The file.
        file: String,
        /// What is missing or what does not match.
        detail: String,
    },
    /// The bundle does not declare the blob policy that the contract requires.
    #[error("the export does not declare the referenced-by-digest policy (policy '{policy}')")]
    BlobPolicy {
        /// What it declares.
        policy: String,
    },
    /// An invalid request from the caller.
    #[error("{0}")]
    Invalid(String),
    /// The store failed.
    #[error("the store failed: {0}")]
    Store(#[from] RecoveryError),
}

/// Canonicalize the identity of a row: `table:key`, the same notion that
/// the export fixes when writing its batches.
///
/// It accepts the two forms in which an id arrives from a JSON:
/// - the string `table:key` (canonical form, the one the contract fixes);
/// - the `{tb, id}` object in which some client serializes a Thing.
///
/// Without an `id` **none is invented**: the identity has to travel in the row;
/// it aborts with `RowInvalid`.
pub fn canonicalize_id(table: &str, row: &Row) -> Result<String, ImportFailure> {
    let declared = row.get("id").ok_or_else(|| ImportFailure::RowInvalid {
        table: table.to_string(),
        id: String::new(),
        detail: "the row carries no id: a canonical identity cannot be invented".to_string(),
    })?;
    match declared {
        serde_json::Value::String(text) => {
            let (prefix, key) = text
                .split_once(':')
                .ok_or_else(|| ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: "the id is not a canonical 'table:key' identity".to_string(),
                })?;
            if prefix != table {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: format!("the id belongs to table '{prefix}'"),
                });
            }
            if key.is_empty() {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: "the id has an empty key".to_string(),
                });
            }
            Ok(text.clone())
        }
        serde_json::Value::Object(object) => {
            let text = text_of(declared);
            let prefix = object
                .get("tb")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text.clone(),
                    detail: "the id object has no table".to_string(),
                })?;
            if prefix != table {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text,
                    detail: format!("the id belongs to table '{prefix}'"),
                });
            }
            let key = object
                .get("id")
                .map(|key| match key {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .ok_or_else(|| ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text,
                    detail: "the id object has no key".to_string(),
                })?;
            if key.is_empty() {
                return Err(ImportFailure::RowInvalid {
                    table: table.to_string(),
                    id: text_of(declared),
                    detail: "the id object has an empty key".to_string(),
                });
            }
            Ok(format!("{table}:{key}"))
        }
        _ => Err(ImportFailure::RowInvalid {
            table: table.to_string(),
            id: text_of(declared),
            detail: "the id is neither a string nor an object".to_string(),
        }),
    }
}

fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Sets the canonical identity in the row before writing: the `id` is
/// always `table:key`, never the origin's UID nor the one that the destination
/// might want to assign.
fn with_canonical_id(mut row: Row, identity: &str) -> Row {
    row.insert(
        "id".to_string(),
        serde_json::Value::String(identity.to_string()),
    );
    row
}

/// Reads and verifies a batch of the bundle: the file has to exist and
/// match the manifest's hash, and its content has to be a
/// list of objects of the declared count.
pub fn read_batch(package: &Path, batch: &ImportBatch) -> Result<Vec<Row>, RecoveryError> {
    let path = package.join(&batch.file);
    let bytes = std::fs::read(&path).map_err(|error| {
        RecoveryError::Metadata(format!(
            "the batch '{}' cannot be read: {error}",
            batch.file
        ))
    })?;
    let hash = sha256_hex(&bytes);
    if hash != batch.sha256 {
        return Err(RecoveryError::Unsound(format!(
            "the batch '{}' is truncated or altered: its bytes hash to {hash}, the manifest \
             says {}",
            batch.file, batch.sha256
        )));
    }
    let rows: Vec<Row> = serde_json::from_slice(&bytes).map_err(|error| {
        RecoveryError::Unsound(format!(
            "the batch '{}' is not a JSON array of rows: {error}",
            batch.file
        ))
    })?;
    if rows.len() as u64 != batch.rows {
        return Err(RecoveryError::Unsound(format!(
            "the batch '{}' carries {} rows, the manifest says {}",
            batch.file,
            rows.len(),
            batch.rows
        )));
    }
    Ok(rows)
}

/// The content digests that a row references, according to
/// [`DIGEST_FIELDS`].
fn referenced_digests(table: &str, row: &Row) -> Vec<String> {
    let mut digests = Vec::new();
    for (name, fields) in DIGEST_FIELDS {
        if *name != table {
            continue;
        }
        for field in *fields {
            if let Some(value) = row.get(*field).and_then(serde_json::Value::as_str)
                && !value.is_empty()
            {
                digests.push(value.to_string());
            }
        }
    }
    digests
}

/// The prior reconciliation: every batch exists, matches its hash, its
/// rows are canonicalized, there are no repeated identities, each identity falls
/// within the bounds of its batch and each referenced digest is in the
/// CAS. None of this touches the destination; it is the module's "manifest-first".
async fn preflight(
    package: &Path,
    manifest: &ImportManifest,
    reconciler: Option<&Arc<dyn DigestReconciler>>,
) -> Result<(), ImportFailure> {
    for table in &manifest.tables {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for batch in &table.batches {
            let rows = read_batch(package, batch).map_err(ImportFailure::Store)?;
            for row in &rows {
                let identity = canonicalize_id(&table.table, row)?;
                if !seen.insert(identity.clone()) {
                    return Err(ImportFailure::DuplicateRow {
                        table: table.table.clone(),
                        key: identity,
                    });
                }
                if identity < batch.first_id || identity > batch.last_id {
                    return Err(ImportFailure::RowOutOfBatch {
                        id: identity,
                        file: batch.file.clone(),
                        first: batch.first_id.clone(),
                        last: batch.last_id.clone(),
                    });
                }
                if let Some(reconciler) = reconciler {
                    for digest in referenced_digests(&table.table, row) {
                        if !reconciler.has(&digest).await? {
                            return Err(ImportFailure::MissingBlobs {
                                missing: 1,
                                first: digest,
                                referenced_by: identity,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Verifies that a batch marked as applied in the progress is truly
/// at the destination, row by row and digest by digest.
async fn verify_done_batch(
    sink: &Arc<dyn RowSink>,
    package: &Path,
    table: &str,
    outcome: &BatchOutcome,
    manifest: &ImportManifest,
) -> Result<(), ImportFailure> {
    let entry = manifest
        .tables
        .iter()
        .find(|t| t.table == outcome.table)
        .and_then(|t| t.batches.get(outcome.index - 1))
        .ok_or_else(|| ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: "the progress names a batch this package does not carry".to_string(),
        })?;
    if entry.file != outcome.file {
        return Err(ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: format!("the progress slot holds '{}'", entry.file),
        });
    }
    let rows = read_batch(package, entry).map_err(ImportFailure::Store)?;
    if outcome.created + outcome.skipped + outcome.overwritten != rows.len() as u64 {
        return Err(ImportFailure::ProgressNotBacked {
            table: table.to_string(),
            file: outcome.file.clone(),
            detail: format!(
                "the progress says {} rows applied, the batch carries {}",
                outcome.created + outcome.skipped + outcome.overwritten,
                rows.len()
            ),
        });
    }
    for row in &rows {
        let identity = canonicalize_id(&outcome.table, row)?;
        let row = with_canonical_id(row.clone(), &identity);
        let expected = sink.fingerprint_row(&row);
        match sink.lookup(&identity).await? {
            None => {
                return Err(ImportFailure::ProgressNotBacked {
                    table: table.to_string(),
                    file: outcome.file.clone(),
                    detail: format!("'{identity}' is not in the destination"),
                });
            }
            Some(stored) => {
                if sink.fingerprint_row(&stored) != expected {
                    return Err(ImportFailure::ProgressNotBacked {
                        table: table.to_string(),
                        file: outcome.file.clone(),
                        detail: format!("'{identity}' differs from what the batch declares"),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Imports an export bundle into an authority. Idempotent by
/// canonical identity, resumable by batch, fail-closed by default.
///
/// - Checks the manifest and reconciles the whole bundle before the
///   first write (format, self-digest, hashes, order, identities,
///   digests against the CAS).
/// - If there is progress without `resume`, it aborts: resuming without saying so could
///   overlap with a run that did not die. With `resume`, each recorded
///   batch is verified against its hash **and against the destination**.
/// - Applies batch by batch in manifest order: an absent identity is
///   created with the origin's identity; a present identity with the same
///   canonical digest is skipped; a present identity with another digest aborts
///   (`Conflict`) unless `overwrite`, which rewrites keeping the
///   identity.
/// - Records progress after each batch atomically and deletes it on finishing.
pub async fn import_export(
    package: &Path,
    sink: Arc<dyn RowSink>,
    reconciler: Option<Arc<dyn DigestReconciler>>,
    options: &ImportOptions,
    progress_path: &Path,
) -> Result<ImportReport, ImportFailure> {
    let started = std::time::Instant::now();
    let manifest = read_import_manifest(package).map_err(ImportFailure::Store)?;
    if manifest.blobs.policy != "referenced-by-digest" || manifest.blobs.copied {
        return Err(ImportFailure::BlobPolicy {
            policy: manifest.blobs.policy.clone(),
        });
    }
    if options.batch_rows == 0 {
        return Err(ImportFailure::Invalid(
            "the page size cannot be zero".to_string(),
        ));
    }
    if reconciler.is_none() && !options.allow_missing_blobs {
        return Err(ImportFailure::Invalid(
            "the import requires a CAS digest reconciler or an explicit \
             allow_missing_blobs waiver (doc-20: a digest the CAS lacks is the \
             import's failure to raise, not its silence)"
                .to_string(),
        ));
    }
    // Manifest-first: nothing is applied until the whole bundle matches.
    preflight(package, &manifest, reconciler.as_ref()).await?;

    let fingerprint_before = sink.state_summary().await?.fingerprint();

    let mut progress: Option<ImportProgress> = None;
    match std::fs::read(progress_path) {
        Ok(text) => {
            let on_disk: ImportProgress = serde_json::from_slice(&text).map_err(|error| {
                ImportFailure::Invalid(format!("the import progress is unreadable: {error}"))
            })?;
            if on_disk.content_sha256 != manifest.content_sha256 {
                return Err(ImportFailure::ProgressMismatch {
                    progress: on_disk.content_sha256,
                    package: manifest.content_sha256.clone(),
                });
            }
            if !options.resume {
                return Err(ImportFailure::Invalid(
                    "the destination holds an unfinished import; pass resume to continue it \
                     (each done batch is re-verified against its hash and the destination \
                     first)"
                        .to_string(),
                ));
            }
            for outcome in &on_disk.done {
                verify_done_batch(&sink, package, &outcome.table, outcome, &manifest).await?;
            }
            progress = Some(on_disk);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ImportFailure::Invalid(format!(
                "the import progress cannot be read: {error}"
            )));
        }
    }

    let done: BTreeSet<(String, usize)> = progress
        .as_ref()
        .map(|state| {
            state
                .done
                .iter()
                .map(|batch| (batch.table.clone(), batch.index))
                .collect()
        })
        .unwrap_or_default();

    let mut report = ImportReport {
        fingerprint_before,
        fingerprint_after: String::new(),
        frontier: manifest.frontier.clone(),
        content_sha256: manifest.content_sha256.clone(),
        batches: Vec::new(),
        created: 0,
        skipped: 0,
        overwritten: 0,
        finished: false,
        started_at: Utc::now(),
        elapsed_ms: 0,
    };
    let mut applied: Vec<BatchOutcome> = progress
        .as_ref()
        .map(|state| state.done.clone())
        .unwrap_or_default();

    for table in &manifest.tables {
        for (index, batch) in table.batches.iter().enumerate() {
            if done.contains(&(table.table.clone(), index + 1)) {
                continue;
            }
            let rows = read_batch(package, batch).map_err(ImportFailure::Store)?;
            let mut outcome = BatchOutcome {
                table: table.table.clone(),
                index: index + 1,
                file: batch.file.clone(),
                created: 0,
                skipped: 0,
                overwritten: 0,
            };
            let mut page: Vec<(String, Row)> = Vec::with_capacity(options.batch_rows);
            for row in rows {
                let identity = canonicalize_id(&table.table, &row)?;
                let row = with_canonical_id(row, &identity);
                match sink.lookup(&identity).await? {
                    None => {
                        page.push((identity, row));
                        outcome.created += 1;
                    }
                    Some(existing) => {
                        let source_digest = sink.fingerprint_row(&row);
                        let destination_digest = sink.fingerprint_row(&existing);
                        if source_digest == destination_digest {
                            outcome.skipped += 1;
                        } else if options.overwrite {
                            sink.put(&identity, &row).await?;
                            outcome.overwritten += 1;
                        } else {
                            return Err(ImportFailure::Conflict {
                                table: table.table.clone(),
                                key: identity,
                                package_digest: source_digest,
                                destination_digest,
                            });
                        }
                    }
                }
                if page.len() >= options.batch_rows {
                    flush(&sink, std::mem::take(&mut page)).await?;
                }
            }
            if !page.is_empty() {
                flush(&sink, page).await?;
            }
            report.created += outcome.created;
            report.skipped += outcome.skipped;
            report.overwritten += outcome.overwritten;
            report.batches.push(outcome.clone());
            applied.push(outcome);
            write_progress(
                progress_path,
                &ImportProgress {
                    format: IMPORT_FORMAT,
                    content_sha256: manifest.content_sha256.clone(),
                    done: applied.clone(),
                },
            )?;
        }
    }

    report.fingerprint_after = sink.state_summary().await?.fingerprint();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    match std::fs::remove_file(progress_path) {
        Ok(()) => report.finished = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => report.finished = true,
        Err(error) => {
            return Err(ImportFailure::Invalid(format!(
                "the import finished but its progress marker could not be removed: {error}"
            )));
        }
    }
    Ok(report)
}

async fn flush(sink: &Arc<dyn RowSink>, page: Vec<(String, Row)>) -> Result<(), ImportFailure> {
    for (identity, row) in page {
        sink.put(&identity, &row).await?;
    }
    Ok(())
}

fn write_progress(path: &Path, state: &ImportProgress) -> Result<(), ImportFailure> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ImportFailure::Invalid(format!("the progress directory cannot be created: {error}"))
        })?;
    }
    let temporary = path.with_file_name(".import-progress.part");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| ImportFailure::Invalid(format!("the progress is unwritable: {error}")))?;
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| {
            ImportFailure::Invalid(format!("the import progress cannot be written: {error}"))
        })
}

/// The in-memory sink for the contract and the tests: a map of
/// `table:identity -> row`, with the same semantics that the graph
/// adapter requires (the canonical identity is the key, UIDs do not
/// exist).
#[derive(Debug, Default)]
pub struct MemorySink {
    rows: tokio::sync::Mutex<BTreeMap<String, Row>>,
    fingerprint_override: Option<fn(&Row) -> String>,
}

impl MemorySink {
    /// An empty one.
    pub fn new() -> Self {
        Self::default()
    }

    /// With its own digest function: the adapter that normalizes the
    /// origin's representation equates here its notion of "same
    /// content".
    pub fn with_fingerprint(digest: fn(&Row) -> String) -> Self {
        Self {
            rows: tokio::sync::Mutex::new(BTreeMap::new()),
            fingerprint_override: Some(digest),
        }
    }

    /// The current rows, by canonical identity.
    pub async fn snapshot(&self) -> BTreeMap<String, Row> {
        self.rows.lock().await.clone()
    }

    /// How many rows there are.
    pub async fn len(&self) -> usize {
        self.rows.lock().await.len()
    }

    /// Whether the destination is empty.
    pub async fn is_empty(&self) -> bool {
        self.rows.lock().await.is_empty()
    }
}

#[async_trait]
impl RowSink for MemorySink {
    async fn lookup(&self, identity: &str) -> Result<Option<Row>, RecoveryError> {
        Ok(self.rows.lock().await.get(identity).cloned())
    }

    async fn put(&self, identity: &str, row: &Row) -> Result<(), RecoveryError> {
        self.rows
            .lock()
            .await
            .insert(identity.to_string(), row.clone());
        Ok(())
    }

    fn fingerprint_row(&self, row: &Row) -> String {
        match self.fingerprint_override {
            Some(custom) => custom(row),
            None => canonical_digest(row),
        }
    }

    async fn state_summary(&self) -> Result<DestinationState, RecoveryError> {
        let guard = self.rows.lock().await;
        let mut tables: BTreeMap<String, u64> = BTreeMap::new();
        let mut rows = BTreeMap::new();
        for (key, row) in guard.iter() {
            let table = key.split(':').next().unwrap_or(key).to_string();
            *tables.entry(table).or_default() += 1;
            rows.insert(key.clone(), self.fingerprint_row(row));
        }
        Ok(DestinationState { tables, rows })
    }
}

/// Reconciles the digests that the rows reference against the CAS.
#[async_trait]
pub trait DigestReconciler: Send + Sync {
    /// `true` if the digest exists in the store with its hash verified.
    async fn has(&self, digest: &str) -> Result<bool, RecoveryError>;
}

/// The test and local-rehearsal reconciler: the set of
/// digests that the store has.
#[derive(Debug, Default)]
pub struct SetReconciler {
    digests: BTreeSet<String>,
}

impl SetReconciler {
    /// One that knows exactly these digests.
    pub fn new<I: IntoIterator<Item = String>>(digests: I) -> Self {
        Self {
            digests: digests.into_iter().collect(),
        }
    }
}

#[async_trait]
impl DigestReconciler for SetReconciler {
    async fn has(&self, digest: &str) -> Result<bool, RecoveryError> {
        Ok(self.digests.contains(digest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(table: &str, key: &str, fields: &[(&str, serde_json::Value)]) -> Row {
        let mut row = Row::new();
        row.insert(
            "id".to_string(),
            serde_json::Value::String(format!("{table}:{key}")),
        );
        for (name, value) in fields {
            row.insert((*name).to_string(), value.clone());
        }
        row
    }

    fn number(value: i64) -> serde_json::Value {
        serde_json::Value::Number(value.into())
    }

    #[test]
    fn a_canonical_string_id_is_the_identity() {
        let row = row("catalog_tag", "repo|v1", &[]);
        assert_eq!(
            canonicalize_id("catalog_tag", &row).unwrap(),
            "catalog_tag:repo|v1"
        );
    }

    #[test]
    fn a_ledger_composite_key_survives_canonicalization() {
        // The ledger contract: the identity is `stream,pos` within the
        // id; the import keeps it literal, it neither reorders nor rewrites it.
        let row = row("event_ledger", "stream_a,0", &[("position", number(0))]);
        assert_eq!(
            canonicalize_id("event_ledger", &row).unwrap(),
            "event_ledger:stream_a,0"
        );
    }

    #[test]
    fn an_object_id_is_canonicalized_to_the_same_text() {
        let mut row = Row::new();
        let mut id = serde_json::Map::new();
        id.insert(
            "tb".to_string(),
            serde_json::Value::String("event_ledger".to_string()),
        );
        id.insert("id".to_string(), number(7));
        row.insert("id".to_string(), serde_json::Value::Object(id));
        assert_eq!(
            canonicalize_id("event_ledger", &row).unwrap(),
            "event_ledger:7"
        );
    }

    #[test]
    fn a_row_without_an_id_is_refused_not_invented() {
        let mut row = Row::new();
        row.insert(
            "note".to_string(),
            serde_json::Value::String("x".to_string()),
        );
        assert!(matches!(
            canonicalize_id("t", &row),
            Err(ImportFailure::RowInvalid { .. })
        ));
    }

    #[test]
    fn an_id_from_another_table_is_refused() {
        let row = row("catalog_tag", "x", &[]);
        assert!(matches!(
            canonicalize_id("catalog_asset", &row),
            Err(ImportFailure::RowInvalid { .. })
        ));
    }

    #[test]
    fn the_content_digest_pins_the_batch_order() {
        let a = ImportBatch {
            file: "batches/x-000001.json".into(),
            sha256: "a".repeat(64),
            rows: 1,
            first_id: "x:1".into(),
            last_id: "x:1".into(),
        };
        let b = ImportBatch {
            file: "batches/x-000002.json".into(),
            sha256: "b".repeat(64),
            rows: 1,
            first_id: "x:2".into(),
            last_id: "x:2".into(),
        };
        let table = |batches: Vec<ImportBatch>| ImportTable {
            table: "x".into(),
            rows: 2,
            batches,
        };
        let forward = manifest_of(vec![table(vec![a.clone(), b.clone()])]);
        let backward = manifest_of(vec![table(vec![b, a])]);
        assert_ne!(forward.content_digest(), backward.content_digest());
    }

    fn manifest_of(tables: Vec<ImportTable>) -> ImportManifest {
        let mut frontier_tables = BTreeMap::new();
        for table in &tables {
            frontier_tables.insert(table.table.clone(), table.rows);
        }
        ImportManifest {
            format: IMPORT_FORMAT,
            tool_version: "test".into(),
            source: ExportSource {
                engine: "surrealdb".into(),
                engine_version: "2.3.9".into(),
                backend: "mem".into(),
                namespace: "hodei".into(),
                database: "artifacts".into(),
            },
            frontier: ExportFrontier {
                tables: frontier_tables,
                ledger_rows: 0,
                ledger_max_position: None,
                head_rows: 0,
            },
            tables,
            blobs: BlobPolicy {
                policy: "referenced-by-digest".into(),
                copied: false,
            },
            content_sha256: String::new(),
        }
    }

    #[test]
    fn an_altered_self_digest_is_refused() {
        let mut manifest = manifest_of(vec![]);
        manifest.content_sha256 = manifest.content_digest();
        manifest
            .check()
            .expect("an empty manifest with its own digest is sound");
        manifest.content_sha256 = "f".repeat(64);
        assert!(matches!(
            manifest.check(),
            Err(ImportFailure::ContentDigest { .. })
        ));
    }

    #[test]
    fn unordered_batches_are_refused() {
        let batch = |index: usize, first: &str, last: &str| ImportBatch {
            file: format!("batches/x-{index:06}.json"),
            sha256: "0".repeat(64),
            rows: 1,
            first_id: first.into(),
            last_id: last.into(),
        };
        let manifest = manifest_of(vec![ImportTable {
            table: "x".into(),
            rows: 2,
            batches: vec![batch(1, "x:2", "x:2"), batch(2, "x:1", "x:1")],
        }]);
        assert!(matches!(manifest.check(), Err(ImportFailure::Order { .. })));
    }

    #[tokio::test]
    async fn the_memory_sink_fingerprint_is_insertion_order_independent() {
        let sink = MemorySink::new();
        sink.put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        sink.put("t:b", &row("t", "b", &[("v", number(2))]))
            .await
            .unwrap();
        let first = sink.state_summary().await.unwrap().fingerprint();
        let other = MemorySink::new();
        other
            .put("t:b", &row("t", "b", &[("v", number(2))]))
            .await
            .unwrap();
        other
            .put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        assert_eq!(first, other.state_summary().await.unwrap().fingerprint());
    }

    #[tokio::test]
    async fn a_changed_value_changes_the_fingerprint() {
        let sink = MemorySink::new();
        sink.put("t:a", &row("t", "a", &[("v", number(1))]))
            .await
            .unwrap();
        let before = sink.state_summary().await.unwrap().fingerprint();
        sink.put("t:a", &row("t", "a", &[("v", number(2))]))
            .await
            .unwrap();
        assert_ne!(before, sink.state_summary().await.unwrap().fingerprint());
    }

    #[test]
    fn key_order_inside_a_row_does_not_change_its_digest() {
        let mut first = Row::new();
        first.insert("b".to_string(), number(2));
        first.insert("a".to_string(), number(1));
        let mut second = Row::new();
        second.insert("a".to_string(), number(1));
        second.insert("b".to_string(), number(2));
        assert_eq!(canonical_digest(&first), canonical_digest(&second));
    }

    #[test]
    fn the_digest_ignores_the_identity_itself() {
        let with_id = row("t", "a", &[("v", number(1))]);
        let mut content = row("t", "ignored", &[("v", number(1))]);
        content.remove("id");
        assert_eq!(canonical_digest(&with_id), canonical_digest(&content));
    }

    #[test]
    fn a_nested_object_orders_its_keys_in_the_digest() {
        let mut outer = Row::new();
        let mut inner = Row::new();
        inner.insert("y".to_string(), number(2));
        inner.insert("x".to_string(), number(1));
        outer.insert("meta".to_string(), serde_json::Value::Object(inner));
        let mut shuffled = Row::new();
        let mut inner = Row::new();
        inner.insert("x".to_string(), number(1));
        inner.insert("y".to_string(), number(2));
        shuffled.insert("meta".to_string(), serde_json::Value::Object(inner));
        assert_eq!(canonical_digest(&outer), canonical_digest(&shuffled));
    }

    #[test]
    fn referenced_digests_follow_the_declared_fields() {
        let asset = row(
            "catalog_asset",
            "a",
            &[("sha256", serde_json::Value::String("sha256:aa".into()))],
        );
        assert_eq!(
            referenced_digests("catalog_asset", &asset),
            vec!["sha256:aa".to_string()]
        );
        assert!(referenced_digests("catalog_tag", &asset).is_empty());
    }
}
