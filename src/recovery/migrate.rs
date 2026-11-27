//! Verified migration of blobs between two content backends (HSP-163, minimal
//! slice: copy and verify, never delete).
//!
//! A migration moves *copies*. The rules, none of them negotiable:
//!
//! - the **destination is the deployment's**, never the request's;
//! - the bytes are read from the origin and hashed **here**: bytes that do not
//!   hash to the digest are never written anywhere, so a damaged origin does
//!   not spread (the remedy is the repair, not the migration);
//! - after the destination stored them they are verified again by re-hashing
//!   what the destination holds; only then is the digest reported `migrated`;
//! - the origin is **never written and never deleted** by a migration. There is
//!   no per-digest read switch in this slice, so deleting an origin copy would
//!   take the blob away from the very store that serves it. Retiring the origin
//!   is a separate, explicit step that this slice does not offer;
//! - a failure half way (the destination fails, the process dies) leaves the
//!   origin exactly as it was and at most a verified or absent destination copy:
//!   the next request finds what is verified (`already_present`) and finishes
//!   the rest, so repeating a migration is the same as running it once;
//! - the cost is bounded: blobs per request, bytes per blob, bytes per request
//!   (what does not fit is `deferred`, to be asked for again), at most
//!   [`MAX_CONCURRENT`] migrations at once, and a timeout on every call;
//! - the garbage collection and the repair are untouched: nothing here deletes
//!   or quarantines, so a hold is irrelevant and a blob that the GC or a repair
//!   removes while it is being read is reported absent or corrupt, never copied.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_evidence::ContentDigest;
use hodei_hub::{BlobError, BlobStore};
use serde::Serialize;

/// The most migrations that run at the same time.
pub const MAX_CONCURRENT: usize = 2;

/// The most blobs one request may name.
pub const MAX_BLOBS_PER_REQUEST: usize = 64;

/// How long a call to either store may take by default.
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// What happened to one digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Moved {
    /// The destination now holds the bytes and they hash to the digest.
    Migrated {
        /// The bytes copied.
        bytes: u64,
    },
    /// The destination already held verified bytes: nothing was read from the
    /// origin and nothing was written.
    AlreadyPresent,
    /// The origin holds no copy.
    SourceAbsent,
    /// The origin holds bytes that do not hash to the digest. Nothing was
    /// written to the destination.
    SourceCorrupt,
    /// The origin could not be read.
    SourceFailed,
    /// The blob is above the ceiling of one blob.
    TooLarge {
        /// The ceiling.
        max: u64,
    },
    /// The request already moved as many bytes as it may: ask again.
    Deferred,
    /// The destination already holds a copy that does not verify. It was left
    /// as it was.
    DestinationDamaged,
    /// The destination failed, or what it holds did not verify afterwards.
    DestinationFailed,
}

impl Moved {
    /// Whether the destination verifiably holds the digest afterwards.
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Migrated { .. } | Self::AlreadyPresent)
    }
}

/// One entry of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The content, `sha256:<hex>`.
    pub digest: String,
    /// What happened to it.
    #[serde(flatten)]
    pub moved: Moved,
}

/// What a migration did, digest by digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    /// One entry per digest asked for, in the order asked.
    pub entries: Vec<Entry>,
    /// Digests copied and verified by this request.
    pub migrated: usize,
    /// Digests that were already verified in the destination.
    pub already_present: usize,
    /// Digests that are not verified in the destination after this request.
    pub pending: usize,
    /// Bytes copied by this request.
    pub bytes_copied: u64,
    /// Always `true`: a migration never deletes or writes the origin.
    pub source_retained: bool,
    /// How long it took, in milliseconds.
    pub elapsed_ms: u64,
}

/// Why a migration did not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    /// More digests than one request may name.
    #[error("a request may name at most {max} digests")]
    TooMany {
        /// The ceiling.
        max: usize,
    },
    /// The migrations that may run at once are running.
    #[error("too many migrations are running")]
    Busy,
    /// The migration task itself failed.
    #[error("the migration failed")]
    Failed,
}

/// The migration of blobs from one store to another.
pub struct Migrator {
    source: Arc<dyn BlobStore>,
    destination: Arc<dyn BlobStore>,
    max_blob_bytes: u64,
    max_request_bytes: u64,
    io_timeout: Duration,
    permits: Arc<tokio::sync::Semaphore>,
}

impl Migrator {
    /// A migrator from `source` to `destination`, moving no blob above
    /// `max_blob_bytes` and no more than `max_request_bytes` per request.
    pub fn new(
        source: Arc<dyn BlobStore>,
        destination: Arc<dyn BlobStore>,
        max_blob_bytes: u64,
        max_request_bytes: u64,
    ) -> Self {
        Self {
            source,
            destination,
            max_blob_bytes,
            max_request_bytes,
            io_timeout: DEFAULT_IO_TIMEOUT,
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
        }
    }

    /// The same migrator with another timeout for each call to a store.
    pub fn with_io_timeout(mut self, io_timeout: Duration) -> Self {
        self.io_timeout = io_timeout;
        self
    }

    /// Migrates `digests` (duplicates are dropped, order is kept).
    ///
    /// The work is spawned and awaited: a client that hangs up does not leave a
    /// copy half done (the stores only ever publish whole, verified blobs, but
    /// the destination is left coherent and the permit is always returned).
    pub async fn migrate(
        &self,
        digests: &[ContentDigest],
    ) -> Result<MigrationReport, MigrateError> {
        let mut unique: Vec<ContentDigest> = Vec::with_capacity(digests.len());
        for digest in digests {
            if !unique.contains(digest) {
                unique.push(digest.clone());
            }
        }
        if unique.len() > MAX_BLOBS_PER_REQUEST {
            return Err(MigrateError::TooMany {
                max: MAX_BLOBS_PER_REQUEST,
            });
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| MigrateError::Busy)?;
        let run = Run {
            source: self.source.clone(),
            destination: self.destination.clone(),
            max_blob_bytes: self.max_blob_bytes,
            max_request_bytes: self.max_request_bytes,
            io_timeout: self.io_timeout,
        };
        let task = tokio::spawn(async move {
            let _permit = permit;
            run.go(&unique).await
        });
        // A task that panicked produced nothing: that is a failure, never an
        // invented report.
        task.await.map_err(|error| {
            tracing::error!(%error, "the migration task failed");
            MigrateError::Failed
        })
    }

    /// The ceiling on the bytes of one blob.
    pub fn max_blob_bytes(&self) -> u64 {
        self.max_blob_bytes
    }
}

impl MigrationReport {
    fn of(entries: Vec<Entry>, bytes_copied: u64, elapsed: Duration) -> Self {
        let migrated = entries
            .iter()
            .filter(|e| matches!(e.moved, Moved::Migrated { .. }))
            .count();
        let already_present = entries
            .iter()
            .filter(|e| e.moved == Moved::AlreadyPresent)
            .count();
        let pending = entries.iter().filter(|e| !e.moved.is_verified()).count();
        Self {
            entries,
            migrated,
            already_present,
            pending,
            bytes_copied,
            source_retained: true,
            elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

struct Run {
    source: Arc<dyn BlobStore>,
    destination: Arc<dyn BlobStore>,
    max_blob_bytes: u64,
    max_request_bytes: u64,
    io_timeout: Duration,
}

impl Run {
    /// One call to a store, bounded in time: a call that times out is a
    /// failure of that store, never a success.
    async fn io<T>(
        &self,
        call: impl Future<Output = Result<T, BlobError>>,
    ) -> Result<T, BlobError> {
        tokio::time::timeout(self.io_timeout, call)
            .await
            .unwrap_or_else(|_| Err(BlobError::Unavailable("the call timed out".to_string())))
    }

    async fn go(&self, digests: &[ContentDigest]) -> MigrationReport {
        let started = Instant::now();
        let mut copied = 0u64;
        let mut entries = Vec::with_capacity(digests.len());
        for digest in digests {
            let moved = self.one(digest, &mut copied).await;
            entries.push(Entry {
                digest: digest.to_string(),
                moved,
            });
        }
        MigrationReport::of(entries, copied, started.elapsed())
    }

    async fn one(&self, digest: &ContentDigest, copied: &mut u64) -> Moved {
        // What the destination already verifies is not copied again.
        match self.io(self.destination.verify(digest)).await {
            Ok(()) => return Moved::AlreadyPresent,
            Err(BlobError::NotFound) => {}
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Moved::DestinationDamaged;
            }
            Err(error) => {
                tracing::error!(%error, "the destination could not be checked");
                return Moved::DestinationFailed;
            }
        }

        let length = match self.io(self.source.length(digest)).await {
            Ok(length) => length,
            Err(BlobError::NotFound) => return Moved::SourceAbsent,
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Moved::SourceCorrupt;
            }
            Err(error) => {
                tracing::error!(%error, "the origin could not be measured");
                return Moved::SourceFailed;
            }
        };
        if length > self.max_blob_bytes {
            return Moved::TooLarge {
                max: self.max_blob_bytes,
            };
        }
        if copied.saturating_add(length) > self.max_request_bytes {
            return Moved::Deferred;
        }

        let bytes = match self.io(self.source.get(digest)).await {
            Ok(bytes) => bytes,
            Err(BlobError::NotFound) => return Moved::SourceAbsent,
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Moved::SourceCorrupt;
            }
            Err(error) => {
                tracing::error!(%error, "the origin could not be read");
                return Moved::SourceFailed;
            }
        };
        // The only check that matters, made here and not delegated to the
        // origin: nothing that does not hash to the digest is ever written.
        if bytes.len() as u64 > self.max_blob_bytes || &ContentDigest::of_bytes(&bytes) != digest {
            return Moved::SourceCorrupt;
        }

        match self.io(self.destination.put(&bytes)).await {
            Ok(stored) if &stored == digest => {}
            Ok(_) => return Moved::DestinationFailed,
            Err(error) => {
                tracing::error!(%error, "the destination could not store the bytes");
                return Moved::DestinationFailed;
            }
        }
        // A copy that is not read back is a claim, not a fact.
        if self.io(self.destination.verify(digest)).await.is_err() {
            return Moved::DestinationFailed;
        }
        *copied += bytes.len() as u64;
        Moved::Migrated {
            bytes: bytes.len() as u64,
        }
    }
}
