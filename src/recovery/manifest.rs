//! The manifest of a backup: what the backup says about itself.
//!
//! It is the last thing written, and what makes it possible to check the whole backup
//! without restoring it: the hash of the metadata, and the digest and length of
//! each blob. A backup without a manifest is not finished and is not restored.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::RecoveryError;

/// The version of the backup format.
///
/// It goes up when an old format can no longer be read. A backup with a
/// format that this binary does not understand is **rejected** with a message, instead
/// of being restored halfway.
pub const FORMAT: u32 = 1;

/// The name of the manifest within the backup.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The metadata file (SurrealQL) within the backup.
pub const METADATA_FILE: &str = "metadata.surql";

/// The blobs directory within the backup, with the same layout as the
/// store: `<ab>/<cd>/<digest hex>`.
pub const BLOBS_DIR: &str = "blobs";

/// The exported metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// The file, relative to the backup.
    pub file: String,
    /// The SHA-256 of the file, in hexadecimal.
    pub sha256: String,
    /// The bytes it occupies.
    pub bytes: u64,
}

/// A blob of the backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobEntry {
    /// The content, as `sha256:<hex>`.
    pub digest: String,
    /// The bytes it occupies.
    pub length: u64,
}

/// A blob that existed and was not copied, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excluded {
    /// The content, as `sha256:<hex>`.
    pub digest: String,
    /// Why it was not copied.
    pub reason: String,
}

/// What a backup says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// The format version.
    pub format: u32,
    /// The version of the service that made it.
    pub service_version: String,
    /// The SurrealDB namespace.
    pub namespace: String,
    /// The SurrealDB database.
    pub database: String,
    /// When the backup started.
    pub started_at: DateTime<Utc>,
    /// The instant the metadata belongs to: everything committed before
    /// this instant is in the backup, and nothing committed after.
    ///
    /// It is the basis of the RPO: what was written between this instant and a
    /// later failure is lost.
    pub metadata_snapshot_at: DateTime<Utc>,
    /// When the backup finished.
    pub finished_at: DateTime<Utc>,
    /// The metadata.
    pub metadata: MetadataEntry,
    /// The copied blobs, sorted by digest.
    pub blobs: Vec<BlobEntry>,
    /// The blobs that were not copied because they were already damaged.
    #[serde(default)]
    pub excluded: Vec<Excluded>,
}

impl BackupManifest {
    /// The bytes of blobs that the backup carries.
    pub fn blob_bytes(&self) -> u64 {
        self.blobs.iter().map(|blob| blob.length).sum()
    }

    /// Checks that the backup can be restored with this binary.
    ///
    /// It does not touch the disk: it is the pure part. It rejects a format that is not
    /// understood and anything that, if accepted, would let a hostile
    /// manifest write outside the backup or the store.
    pub fn check_restorable(&self) -> Result<(), RecoveryError> {
        if self.format != FORMAT {
            return Err(RecoveryError::Unsound(format!(
                "the backup has format {} and this build understands format {FORMAT}",
                self.format
            )));
        }
        if self.metadata.file != METADATA_FILE {
            return Err(RecoveryError::Unsound(format!(
                "the metadata file must be {METADATA_FILE}, not {}",
                self.metadata.file
            )));
        }
        if !is_hex(&self.metadata.sha256, 64) {
            return Err(RecoveryError::Unsound(
                "the hash of the metadata is not a sha256".to_string(),
            ));
        }
        let mut previous: Option<&str> = None;
        for blob in &self.blobs {
            if hex_of(&blob.digest).is_none() {
                return Err(RecoveryError::Unsound(format!(
                    "the digest {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    blob.digest
                )));
            }
            if previous.is_some_and(|last| last >= blob.digest.as_str()) {
                return Err(RecoveryError::Unsound(format!(
                    "the blob {} is repeated or out of order",
                    blob.digest
                )));
            }
            previous = Some(&blob.digest);
        }
        Ok(())
    }
}

/// The hex of a well-formed `sha256:<hex>` digest.
pub fn hex_of(digest: &str) -> Option<&str> {
    let hex = digest.strip_prefix("sha256:")?;
    is_hex(hex, 64).then_some(hex)
}

fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: char) -> String {
        format!("sha256:{}", seed.to_string().repeat(64))
    }

    fn manifest() -> BackupManifest {
        let now = Utc::now();
        BackupManifest {
            format: FORMAT,
            service_version: "0.1.0".into(),
            namespace: "hodei".into(),
            database: "artifacts".into(),
            started_at: now,
            metadata_snapshot_at: now,
            finished_at: now,
            metadata: MetadataEntry {
                file: METADATA_FILE.into(),
                sha256: "a".repeat(64),
                bytes: 10,
            },
            blobs: vec![
                BlobEntry {
                    digest: digest('a'),
                    length: 3,
                },
                BlobEntry {
                    digest: digest('b'),
                    length: 4,
                },
            ],
            excluded: vec![],
        }
    }

    #[test]
    fn a_well_formed_manifest_is_restorable() {
        assert!(manifest().check_restorable().is_ok());
        assert_eq!(manifest().blob_bytes(), 7);
    }

    #[test]
    fn a_format_this_build_does_not_understand_is_refused_with_both_numbers() {
        let mut other = manifest();
        other.format = FORMAT + 1;
        let error = other.check_restorable().unwrap_err().to_string();
        assert!(error.contains(&format!("format {}", FORMAT + 1)), "{error}");
        assert!(error.contains(&format!("format {FORMAT}")), "{error}");
    }

    #[test]
    fn a_digest_that_could_escape_the_store_is_refused() {
        for hostile in ["sha256:../../etc/passwd", "sha256:AB", "md5:abc", "../x"] {
            let mut other = manifest();
            other.blobs[0].digest = hostile.into();
            assert!(
                other.check_restorable().is_err(),
                "{hostile} must be refused"
            );
        }
    }

    #[test]
    fn a_repeated_or_unordered_blob_is_refused() {
        let mut repeated = manifest();
        repeated.blobs[1].digest = digest('a');
        assert!(repeated.check_restorable().is_err());
        let mut unordered = manifest();
        unordered.blobs.reverse();
        assert!(unordered.check_restorable().is_err());
    }

    #[test]
    fn the_metadata_file_cannot_point_somewhere_else() {
        let mut other = manifest();
        other.metadata.file = "../outside.surql".into();
        assert!(other.check_restorable().is_err());
    }

    #[test]
    fn the_manifest_survives_a_round_trip_through_json() {
        let original = manifest();
        let text = serde_json::to_string_pretty(&original).unwrap();
        let back: BackupManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(back, original);
    }
}
