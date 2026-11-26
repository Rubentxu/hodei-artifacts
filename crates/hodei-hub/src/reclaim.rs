//! Reclaiming CAS bytes: what the collector needs from the
//! store, and nothing more.
//!
//! The collector decides **what** can be deleted by looking at the catalog; the
//! store is the one that knows **when** a blob was last written or reused,
//! and the one that can delete without trampling a concurrent write.
//! That is why deletion is not `rm`: it is `delete_if_older_than`, which
//! checks the age **inside the store's barrier**, the same one that
//! protects an upload's commit.
//!
//! # The barrier
//!
//! An upload that ends with content the store already has writes
//! nothing: it reuses the blob. Without a mark, a collector that
//! has just decided that blob is an orphan would delete it right before
//! the catalog references it. The mark is the **modification time
//! of the blob**: every commit or `put` that reuses a blob renews it, and
//! deletion reads it again before acting. Both steps run under the
//! same lock, so there is no gap between looking and deleting.

use std::time::Duration;

use async_trait::async_trait;
use hodei_evidence::ContentDigest;

use crate::blob::BlobError;

/// A blob on disk, as the collector sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes it occupies.
    pub length: u64,
    /// How long ago it was last written or reused.
    pub age: Duration,
}

/// What happened when trying to delete a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reclaimed {
    /// The blob was deleted.
    Deleted {
        /// The bytes it occupied.
        length: u64,
    },
    /// The blob was no longer there: another pass, or a restart, deleted it before.
    Gone,
    /// The blob was reused after deciding it was an orphan: it stays.
    Fresh,
}

/// What the collector needs from the byte store.
#[async_trait]
pub trait BlobReclaim: Send + Sync {
    /// All the blobs the store has, with their age.
    ///
    /// It only lists what the store recognizes as its own: a file with
    /// another name, or in another place, is not a blob and is not listed, so
    /// it cannot be deleted either.
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError>;

    /// Deletes the blob **only if** it has gone untouched for at least `grace`.
    ///
    /// The check and the deletion are atomic with respect to commits to the
    /// same store.
    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        grace: Duration,
    ) -> Result<Reclaimed, BlobError>;
}

/// What the store says about a conditional action on an exact version
/// of a blob (HSP-192, AC2).
///
/// The condition is the mark of the version the collector observed
/// when marking: `delete_if_untouched` deletes only if the current version is still
/// the marked one; `quarantine_if_untouched` copies the marked version
/// to a NEW name and only then, under the same condition, removes
/// the original key. If the condition is not met, the store touches
/// nothing and says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalOutcome {
    /// The marked version was the current one: the action was applied.
    Applied {
        /// The bytes of the acted-upon version.
        length: u64,
    },
    /// The blob was no longer there: another pass acted on it first.
    Gone,
    /// The version changed since the mark: **nothing was touched**. The original
    /// key belongs to whoever rewrote it and is left intact.
    NotApplied,
}

/// The mark of a blob version: what identifies "this exact version"
/// for the conditional guards.
///
/// In a file store it is the pair (mtime, size); in S3 it is the
/// ETag of the latest version. Two concurrent writes of different
/// content produce different marks; two writes of the same
/// content may produce the same mark, and that is acceptable: the
/// guard protects against trampling a version the collector did not see,
/// not against the idempotent rewrite of the same byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionMarker {
    /// The object's opaque key as the store sees it.
    pub key: String,
    /// The version's mark (ETag, mtime+nano, size...).
    pub marker: String,
    /// The bytes of the version.
    pub length: u64,
}

/// The capability of an object store for **safe conditional delete**.
///
/// The coordinated collector requires it before deleting a mark: if the
/// store does not have it, the pass does NOT delete the original key --it forbids
/// its reuse until reconciled-- or quarantines by versioning (new
/// name, never rewriting). This question is the one AC2 answers, and
/// that is why it lives in the store's type, not in a configuration that
/// can be set wrongly.
#[async_trait]
pub trait ConditionalBlob: Send + Sync {
    /// Whether this store can delete or quarantine an exact version
    /// without a possible race with a concurrent write.
    fn supports_conditional_delete(&self) -> bool;

    /// Reads the mark of the blob's current version, if it exists.
    async fn head_marker(&self, digest: &ContentDigest)
    -> Result<Option<VersionMarker>, BlobError>;

    /// The versioned quarantine name of this blob for the pass
    /// `run_id`: a NEW name that the original key does not occupy, and that
    /// two passes do not share. Each store derives it its own way (on
    /// disk a path under `quarantine/`, in S3 a key under the
    /// `quarantine/` prefix).
    fn quarantine_key_of(&self, digest: &ContentDigest, run_id: &str) -> String {
        format!("quarantine/{run_id}/{}", digest.value)
    }

    /// Deletes the blob key only if its current version is still marked
    /// by `marker`. It never deletes a version the collector did not see.
    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError>;

    /// Versioned quarantine: copies the marked version to `quarantine_key`
    /// (a NEW name that the original key does not occupy) and, only if the
    /// copy is exact, removes the original key under the condition of the
    /// `marker`.
    ///
    /// Quarantine is the AC2 alternative when conditional delete
    /// does not exist or the race cannot be closed: the marked
    /// bytes remain accessible under the new name, and the original key
    /// is no longer controlled by the collector.
    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError>;
}
