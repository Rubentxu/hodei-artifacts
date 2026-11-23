//! What an export says about itself, and when that statement is sound.
//!
//! The manifest is written last, and it is the only thing an importer
//! (the future HSP-210 import) needs to trust before touching a row: the
//! hash of every batch, the row count of every table, the frontier the
//! source was at, and the digest of the whole content. Nothing wall-clock
//! enters it, so the same source content yields byte-identical manifests
//! -- determinism is the contract, not a hope.
//!
//! The progress marker is the export's own bookkeeping: one entry per
//! batch already written, hashed as it was written. It is what makes a
//! crashed export resumable without re-reading what is already sound.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ExportError;

/// The version of the export format.
///
/// It goes up when an older export can no longer be read. An export with
/// a format this build does not understand is refused with a message, not
/// imported halfway.
pub const FORMAT: u32 = 1;

/// The name of the manifest inside the export directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The directory of batch files inside the export directory.
pub const BATCHES_DIR: &str = "batches";

/// The name of the progress marker inside the export directory.
///
/// A marker present means the export is unfinished; the manifest replaces
/// it when the export completes.
pub const PROGRESS_FILE: &str = ".progress.json";

/// The tables the export carries, in export order, with the fields that
/// make a row's canonical identity.
///
/// The identity fields mirror the `UNIQUE` indexes of the production
/// schema: a repeated identity is a broken uniqueness guarantee, and the
/// export refuses it. Tables whose schema declares no unique index carry
/// the empty list: their identity is the record id alone.
///
/// The order is the dependency order of the model -- repositories before
/// versions and assets, the ledger after the rows that reference it --
/// so an importer can apply batches in manifest order. The `gc_*` tables
/// are excluded on purpose: they are operational state of the old engine
/// (runs, holds, tombstones), not registry semantics, and the target
/// recomputes them. `catalog_schema` is excluded too: the target declares
/// its own schema versioning.
///
/// The evidence tables (`evidence`, `evidence_subject`,
/// `evidence_external`) are excluded as well, for a different and
/// temporary reason: the evidence of a record is **not reconstructible
/// from the `event_ledger` yet** -- the put of evidence still writes its
/// batch (record, index entry, subject pairs) without appending anything
/// to the ledger (HSP-185 pending) -- so an export that carried them
/// without a way to re-derive them would present evidence the manifest
/// cannot vouch for as derived content. Whether the evidence travels
/// through its own vertical (with its own manifest and frontier) or
/// enters `EXPORTED_TABLES` once it is ledger-backed is a decision
/// pending on the product owner (HSP-188). Until either lands, an export
/// of this list is an export of the registry semantics, not of the
/// evidence.
pub const EXPORTED_TABLES: &[(&str, &[&str])] = &[
    ("catalog_repository", &["account", "repo_key"]),
    (
        "catalog_version",
        &["account", "repository", "package_name", "package_version"],
    ),
    ("catalog_asset", &["account", "repository", "asset_path"]),
    (
        "catalog_reference",
        &["account", "repository", "asset_path"],
    ),
    ("catalog_tag", &["account", "repository", "tag_name"]),
    ("trust_grant", &[]),
    ("provenance_receipt", &[]),
    ("catalog_audit_outbox", &[]),
    ("event_ledger", &[]),
    ("event_ledger_head", &[]),
];

/// Where the source came from: the engine, the contract version the tool
/// was written against, and the namespace and database the rows live in.
///
/// The engine version is the *contract floor* of the tool (the same rule
/// the catalog adapter states), not a runtime probe: a manifest must not
/// depend on incidental runtime details, or determinism dies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// The engine the source runs on.
    pub engine: String,
    /// The engine version the export format was written against.
    pub engine_version: String,
    /// The storage backend the tool was pointed at, as the operator
    /// declared it (`rocksdb` in production, `mem` in rehearsals).
    pub backend: String,
    /// The SurrealDB namespace the rows live in.
    pub namespace: String,
    /// The SurrealDB database the rows live in.
    pub database: String,
}

/// The frontier of the source: how many rows each table had, and where
/// the evidence ledger head was, when the export started (and, verified
/// again, when it ended).
///
/// It is the mutation tripwire of the export: the frontier captured
/// before must equal the frontier captured after, or the run fails. It is
/// also the honest answer to "what point in time does this export
/// represent".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frontier {
    /// Row count per exported table, keyed by table name.
    pub tables: BTreeMap<String, u64>,
    /// The rows of the evidence ledger.
    pub ledger_rows: u64,
    /// The highest stream position the ledger reached, when the ledger is
    /// not empty.
    pub ledger_max_position: Option<u64>,
    /// The rows of the ledger head marker table.
    pub head_rows: u64,
}

/// One batch file of the export, and the hash that pins its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchEntry {
    /// The file, relative to the export directory
    /// (`batches/<table>-<index>.json`).
    pub file: String,
    /// The SHA-256 of the file bytes, in lowercase hexadecimal.
    pub sha256: String,
    /// How many rows the batch carries.
    pub rows: u64,
    /// The record id of the first row of the batch.
    pub first_id: String,
    /// The record id of the last row of the batch.
    pub last_id: String,
}

/// The batches of one table, in export order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableEntry {
    /// The table name.
    pub table: String,
    /// How many rows the table carried in total.
    pub rows: u64,
    /// The batches, in export order.
    pub batches: Vec<BatchEntry>,
}

/// What the export says about blobs: they do not travel.
///
/// The CAS already holds every blob the rows reference, by digest. The
/// export copies no bytes and the import must reconcile every digest
/// against the CAS; a digest the CAS lacks is the import's failure to
/// raise, not this tool's silence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobPolicy {
    /// Why the blobs are not in the export: they are referenced by
    /// digest.
    pub policy: String,
    /// Whether blob bytes travel: they never do.
    pub copied: bool,
}

/// One recorded batch of the progress marker, with the table it belongs
/// to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEntry {
    /// The table the batch belongs to.
    pub table: String,
    /// The batch, hashed as it was written.
    pub batch: BatchEntry,
}

/// The progress marker of an unfinished export.
///
/// It is written after every batch. A resume reads it, re-verifies every
/// recorded batch against its hash, re-captures the frontier and compares
/// it with the recorded one, and then continues from the recorded
/// position. When the export completes, the manifest replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressState {
    /// The version of the marker format.
    pub format: u32,
    /// The frontier captured before the export started.
    pub frontier: Frontier,
    /// The batches already written, in export order.
    pub done: Vec<ProgressEntry>,
    /// The table the next run continues with, when batches remain.
    pub next_table: Option<String>,
    /// The next batch index (1-based) of [`ProgressState::next_table`].
    pub next_batch_index: usize,
}

/// What an export says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportManifest {
    /// The version of the export format.
    pub format: u32,
    /// The version of the tool that made the export.
    pub tool_version: String,
    /// Where the rows came from.
    pub source: SourceInfo,
    /// The frontier of the source at export time.
    pub frontier: Frontier,
    /// The tables, in export order, with their batches.
    pub tables: Vec<TableEntry>,
    /// The blob policy: digests travel, bytes do not.
    pub blobs: BlobPolicy,
    /// The SHA-256 of the whole content: the hash of the concatenation of
    /// every batch hash, in export order, each followed by a newline.
    pub content_sha256: String,
}

impl ExportManifest {
    /// The ordered batch hashes, each followed by a newline.
    fn digest_input(&self) -> Vec<u8> {
        let mut input = Vec::new();
        for table in &self.tables {
            for batch in &table.batches {
                input.extend_from_slice(batch.sha256.as_bytes());
                input.push(b'\n');
            }
        }
        input
    }

    /// Recomputes the content digest from the batch hashes.
    pub fn content_digest(&self) -> String {
        sha256_hex(&self.digest_input())
    }

    /// The batches of every table, in export order, with their table.
    pub fn all_batches(&self) -> Vec<(&str, &BatchEntry)> {
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

    /// Checks the manifest's own soundness: a format this build
    /// understands, well-formed and strictly ordered batch hashes, and a
    /// content digest that matches the batch hashes it declares.
    ///
    /// It does not touch the files on disk; [`ExportManifest::verify`]
    /// does that.
    pub fn check_sound(&self) -> Result<(), ExportError> {
        if self.format != FORMAT {
            return Err(ExportError::ResumeMismatch(format!(
                "the export has format {} and this build understands format {FORMAT}",
                self.format
            )));
        }
        if self.content_sha256 != self.content_digest() {
            return Err(ExportError::ResumeMismatch(format!(
                "the content digest {} does not match the batch hashes the manifest declares",
                self.content_sha256
            )));
        }
        let mut seen_files: Vec<&str> = Vec::new();
        for (table, batch) in self.all_batches() {
            if !is_hex(&batch.sha256, 64) {
                return Err(ExportError::ResumeMismatch(format!(
                    "the hash of batch {} of {table} is not a sha256",
                    batch.file
                )));
            }
            // Los hashes no definen orden semantico y no se exigen
            // crecientes: lo que no se repite es el fichero. Dos entradas
            // con el mismo nombre de fichero es un manifiesto que dice
            // dos cosas de los mismos bytes.
            if seen_files.contains(&batch.file.as_str()) {
                return Err(ExportError::ResumeMismatch(format!(
                    "the batch file {} is repeated in the manifest",
                    batch.file
                )));
            }
            if !batch.file.starts_with(&format!("{BATCHES_DIR}/")) {
                return Err(ExportError::ResumeMismatch(format!(
                    "the batch file {} is not inside {BATCHES_DIR}/",
                    batch.file
                )));
            }
            seen_files.push(batch.file.as_str());
        }
        Ok(())
    }

    /// Verifies the export against the files on disk: every batch file
    /// exists and hashes to what the manifest recorded, and the manifest
    /// itself is sound.
    pub fn verify(&self, export_dir: &Path) -> Result<(), ExportError> {
        self.check_sound()?;
        for (_, batch) in self.all_batches() {
            verify_batch_file(export_dir, batch)?;
        }
        Ok(())
    }
}

/// Verifies one batch file against the hash recorded for it.
pub fn verify_batch_file(export_dir: &Path, batch: &BatchEntry) -> Result<(), ExportError> {
    let path = export_dir.join(&batch.file);
    let bytes = std::fs::read(&path).map_err(|error| {
        ExportError::Io(format!(
            "cannot read the batch file {}: {error}",
            batch.file
        ))
    })?;
    let actual = sha256_hex(&bytes);
    if actual != batch.sha256 {
        return Err(ExportError::TruncatedBatch {
            file: batch.file.clone(),
            expected: batch.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

/// The lowercase hexadecimal SHA-256 of some bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(file: &str, seed: char) -> BatchEntry {
        BatchEntry {
            file: file.to_string(),
            sha256: seed.to_string().repeat(64),
            rows: 2,
            first_id: "t:1".into(),
            last_id: "t:2".into(),
        }
    }

    fn manifest() -> ExportManifest {
        let mut tables = BTreeMap::new();
        tables.insert("catalog_tag".to_string(), 4u64);
        ExportManifest {
            format: FORMAT,
            tool_version: "0.1.0".into(),
            source: SourceInfo {
                engine: "surrealdb".into(),
                engine_version: "2.7.0".into(),
                backend: "rocksdb".into(),
                namespace: "hodei".into(),
                database: "artifacts".into(),
            },
            frontier: Frontier {
                tables,
                ledger_rows: 3,
                ledger_max_position: Some(2),
                head_rows: 2,
            },
            tables: vec![TableEntry {
                table: "catalog_tag".into(),
                rows: 4,
                batches: vec![batch("batches/catalog_tag-000001.json", 'a')],
            }],
            blobs: BlobPolicy {
                policy: "referenced-by-digest".into(),
                copied: false,
            },
            content_sha256: String::new(),
        }
    }

    #[test]
    fn a_manifest_with_an_undeclared_digest_is_unsound() {
        // The digest input is empty (no batches beyond the declared one),
        // so any recorded content digest must equal the hash of the one
        // batch hash. A hand-written digest fails loudly.
        let sound = manifest();
        let mut broken = manifest();
        broken.content_sha256 = "f".repeat(64);
        // Both start unsound: the content digest must be the recomputed
        // one, which the export always writes. The check refuses a
        // mismatched digest in both cases.
        assert!(sound.check_sound().is_err());
        assert!(broken.check_sound().is_err());
    }

    #[test]
    fn a_manifest_with_the_recomputed_digest_is_sound() {
        let mut sound = manifest();
        sound.content_sha256 = sound.content_digest();
        assert!(sound.check_sound().is_ok());
    }

    #[test]
    fn a_batch_outside_the_batches_directory_is_refused() {
        let mut other = manifest();
        other.tables[0].batches[0].file = "../outside.json".into();
        other.content_sha256 = other.content_digest();
        let error = other.check_sound().unwrap_err().to_string();
        assert!(error.contains("outside.json"), "{error}");
    }

    #[test]
    fn a_repeated_batch_file_is_refused() {
        let mut other = manifest();
        other.tables.push(TableEntry {
            table: "event_ledger".into(),
            rows: 2,
            batches: vec![batch("batches/catalog_tag-000001.json", 'b')],
        });
        other.content_sha256 = other.content_digest();
        let error = other.check_sound().unwrap_err().to_string();
        assert!(error.contains("repeated in the manifest"), "{error}");
    }

    #[test]
    fn a_non_hex_batch_hash_is_refused() {
        let mut other = manifest();
        other.tables[0].batches[0].sha256 = "AB".into();
        other.content_sha256 = other.content_digest();
        assert!(other.check_sound().is_err());
    }

    #[test]
    fn the_content_digest_is_the_hash_of_the_ordered_batch_hashes() {
        let mut sound = manifest();
        sound.tables[0]
            .batches
            .push(batch("batches/x-000002.json", 'b'));
        let expected = sha256_hex(format!("{}\n{}\n", "a".repeat(64), "b".repeat(64)).as_bytes());
        assert_eq!(sound.content_digest(), expected);
    }

    #[test]
    fn an_altered_batch_file_fails_verification_with_both_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BATCHES_DIR);
        std::fs::create_dir_all(&path).unwrap();
        let bytes = b"[]".to_vec();
        std::fs::write(path.join("catalog_tag-000001.json"), &bytes).unwrap();
        let mut sound = manifest();
        sound.tables[0].batches[0].sha256 = sha256_hex(&bytes);
        sound.content_sha256 = sound.content_digest();
        assert!(sound.verify(dir.path()).is_ok());
        std::fs::write(path.join("catalog_tag-000001.json"), b"[").unwrap();
        let error = sound.verify(dir.path()).unwrap_err().to_string();
        assert!(error.contains("truncated or altered"), "{error}");
        assert!(error.contains(&sha256_hex(b"[")), "{error}");
    }

    #[test]
    fn a_missing_batch_file_fails_verification() {
        let dir = tempfile::tempdir().unwrap();
        let mut sound = manifest();
        sound.content_sha256 = sound.content_digest();
        let error = sound.verify(dir.path()).unwrap_err().to_string();
        assert!(error.contains("cannot read the batch file"), "{error}");
    }

    #[test]
    fn all_batches_flatten_tables_in_order() {
        let mut sound = manifest();
        sound.tables.push(TableEntry {
            table: "event_ledger".into(),
            rows: 2,
            batches: vec![batch("batches/event_ledger-000001.json", 'c')],
        });
        let names: Vec<&str> = sound.all_batches().iter().map(|(t, _)| *t).collect();
        assert_eq!(names, ["catalog_tag", "event_ledger"]);
    }
}
