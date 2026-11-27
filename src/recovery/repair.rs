//! Self-healing of one blob of the content store (HSP-164, minimal slice).
//!
//! The scrub *finds* a corrupt or missing blob; this module gives it back its
//! bytes. The rules are few and none of them is negotiable:
//!
//! - the bytes come only from the **repair source** the deployment configured
//!   (a replica or the blob directory of a backup), never from the request;
//! - nothing is written until the candidate bytes have been hashed here and
//!   equal the digest asked for: a source that lies, or that is itself damaged,
//!   is rejected and the store is left exactly as it was;
//! - a blob that verifies is **never** touched, whatever the source holds;
//! - a corrupt blob is not deleted: it is moved to a quarantine name (kept for
//!   forensics) only if it is still the very version that was observed, and the
//!   store then publishes the verified bytes under the digest;
//! - repairs are serialised **per digest** (never globally: one account's slow
//!   source cannot hold another's repair), at most [`MAX_CONCURRENT`] at once,
//!   every call to the source or the store has a timeout, and the ceiling on
//!   bytes applies to the store's own blob too: a blob above the ceiling is not
//!   read, verified or repaired (413);
//! - once started, a repair runs to completion even if the client goes away:
//!   the work is spawned, so a dropped request never leaves a quarantined blob
//!   with nothing published in its place;
//! - repairing twice is the same as repairing once: the second finds a sound
//!   blob and does nothing.
//!
//! Any failure leaves the answer "not repaired" and says why. It never invents
//! bytes and never reports a repair it did not verify afterwards.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hodei_evidence::ContentDigest;
use hodei_hub::reclaim::ConditionalOutcome;
use hodei_hub::{BlobError, BlobStore};
use serde::Serialize;

/// The most repairs that run at the same time.
pub const MAX_CONCURRENT: usize = 4;

/// How long a call to the source or the store may take by default.
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// What was wrong with the blob that was repaired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Was {
    /// The bytes no longer hashed to the digest of their name.
    Corrupt,
    /// There were no bytes.
    Absent,
}

/// What a repair did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RepairOutcome {
    /// The blob already verified: nothing was read from the source and
    /// nothing was written.
    AlreadySound,
    /// The blob was given its bytes back and verifies now.
    Repaired {
        /// What was wrong with it.
        was: Was,
        /// The bytes that were written.
        bytes_written: u64,
        /// Whether the damaged bytes were kept under a quarantine name.
        quarantined: bool,
        /// How long the repair took, in milliseconds.
        elapsed_ms: u64,
    },
}

/// Why a blob was not repaired. In every case the store was not made worse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepairError {
    /// The source does not hold the content: there is nothing to repair from.
    #[error("the repair source holds no copy of the content")]
    SourceUnavailable,
    /// The source returned bytes that do not hash to the digest. Nothing was
    /// written.
    #[error("the repair source holds bytes that do not match the digest")]
    SourceMismatch,
    /// The source could not be read.
    #[error("the repair source could not be read")]
    SourceFailed,
    /// The content is larger than the ceiling of one repair.
    #[error("the content is larger than the repair ceiling of {max} bytes")]
    TooLarge {
        /// The ceiling.
        max: u64,
    },
    /// The store cannot quarantine a damaged blob, so it cannot be replaced
    /// safely.
    #[error("the content store cannot replace a damaged blob safely")]
    Unsupported,
    /// The damaged version changed while it was being repaired: another writer
    /// owns it now and nothing was touched.
    #[error("the blob changed while it was being repaired")]
    Raced,
    /// The source or the store did not answer in time; nothing is claimed.
    #[error("the repair source or the content store timed out")]
    TimedOut,
    /// The store failed, or the repaired blob did not verify afterwards.
    #[error("the content store failed during the repair")]
    StoreFailed,
}

/// The repair of one blob.
pub struct Repairer {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<dyn BlobStore>,
    source: Arc<dyn BlobStore>,
    max_bytes: u64,
    io_timeout: Duration,
    /// The digests being repaired: each has its own lock, so only repairs of
    /// the same digest wait for each other.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Bounds the repairs in flight (and so the bytes held in memory).
    permits: tokio::sync::Semaphore,
}

/// Removes the per-digest lock entry when nobody else is waiting on it.
struct LockEntry<'a> {
    inner: &'a Inner,
    key: String,
}

impl Drop for LockEntry<'_> {
    fn drop(&mut self) {
        if let Ok(mut locks) = self.inner.locks.lock()
            && let Some(lock) = locks.get(&self.key)
            && Arc::strong_count(lock) <= 2
        {
            locks.remove(&self.key);
        }
    }
}

impl Repairer {
    /// A repairer of `store` that takes bytes only from `source`, never more
    /// than `max_bytes` for one blob.
    pub fn new(store: Arc<dyn BlobStore>, source: Arc<dyn BlobStore>, max_bytes: u64) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                source,
                max_bytes,
                io_timeout: DEFAULT_IO_TIMEOUT,
                locks: Mutex::new(HashMap::new()),
                permits: tokio::sync::Semaphore::new(MAX_CONCURRENT),
            }),
        }
    }

    /// The same repairer with another timeout for each call to the source or
    /// the store.
    pub fn with_io_timeout(self, io_timeout: Duration) -> Self {
        let old = &self.inner;
        Self {
            inner: Arc::new(Inner {
                store: old.store.clone(),
                source: old.source.clone(),
                max_bytes: old.max_bytes,
                io_timeout,
                locks: Mutex::new(HashMap::new()),
                permits: tokio::sync::Semaphore::new(MAX_CONCURRENT),
            }),
        }
    }

    /// The ceiling on the bytes of one repair.
    pub fn max_bytes(&self) -> u64 {
        self.inner.max_bytes
    }

    /// Repairs the blob named by `digest`, if it needs it.
    ///
    /// The work is spawned and awaited: if the caller is dropped (the client
    /// hung up) the repair still finishes, so the store is never left between
    /// the quarantine of the damaged blob and the publication of the good one.
    pub async fn repair(&self, digest: &ContentDigest) -> Result<RepairOutcome, RepairError> {
        let inner = self.inner.clone();
        let digest = digest.clone();
        tokio::spawn(async move { inner.run(&digest).await })
            .await
            .map_err(|error| {
                tracing::error!(%error, "the repair task failed");
                RepairError::StoreFailed
            })?
    }
}

impl Inner {
    /// One call to the source or the store, bounded in time.
    async fn io<T>(&self, call: impl Future<Output = T>) -> Result<T, RepairError> {
        tokio::time::timeout(self.io_timeout, call)
            .await
            .map_err(|_| RepairError::TimedOut)
    }

    async fn run(&self, digest: &ContentDigest) -> Result<RepairOutcome, RepairError> {
        let started = Instant::now();
        let _permit = self
            .io(self.permits.acquire())
            .await?
            .map_err(|_| RepairError::StoreFailed)?;
        let key = digest.to_string();
        let lock = {
            let mut locks = self.locks.lock().map_err(|_| RepairError::StoreFailed)?;
            locks.entry(key.clone()).or_default().clone()
        };
        let _entry = LockEntry { inner: self, key };
        let _same_digest = self.io(lock.lock()).await?;

        // The ceiling applies to the store's own blob first: its length is
        // read from metadata, and nothing above the ceiling is ever read or
        // verified (a sound giant is as refused as a damaged one).
        let present = match self.io(self.store.length(digest)).await? {
            Ok(length) if length > self.max_bytes => {
                return Err(RepairError::TooLarge {
                    max: self.max_bytes,
                });
            }
            Ok(_) => true,
            Err(BlobError::NotFound) => false,
            Err(error) => {
                tracing::error!(%error, "the repair could not measure the blob");
                return Err(RepairError::StoreFailed);
            }
        };

        // The version that is observed is pinned *before* it is judged, so the
        // version judged damaged is the only one that can be moved away.
        let marker = match self.store.conditional() {
            Some(conditional) if present && conditional.supports_conditional_delete() => {
                match self.io(conditional.head_marker(digest)).await? {
                    Ok(marker) => marker,
                    Err(error) => {
                        tracing::error!(%error, "the repair could not read the blob version");
                        return Err(RepairError::StoreFailed);
                    }
                }
            }
            _ => None,
        };

        let verdict = if present {
            self.io(self.store.verify(digest)).await?
        } else {
            Err(BlobError::NotFound)
        };
        let was = match verdict {
            Ok(()) => return Ok(RepairOutcome::AlreadySound),
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => Was::Corrupt,
            Err(BlobError::NotFound) => Was::Absent,
            Err(error) => {
                tracing::error!(%error, "the repair could not judge the blob");
                return Err(RepairError::StoreFailed);
            }
        };

        // The ceiling is checked before any byte is moved.
        match self.io(self.source.length(digest)).await? {
            Ok(length) if length > self.max_bytes => {
                return Err(RepairError::TooLarge {
                    max: self.max_bytes,
                });
            }
            Ok(_) => {}
            Err(BlobError::NotFound) => return Err(RepairError::SourceUnavailable),
            Err(error) => {
                tracing::error!(%error, "the repair source could not be measured");
                return Err(RepairError::SourceFailed);
            }
        }
        let bytes = match self.io(self.source.get(digest)).await? {
            Ok(bytes) => bytes,
            Err(BlobError::NotFound) => return Err(RepairError::SourceUnavailable),
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Err(RepairError::SourceMismatch);
            }
            Err(error) => {
                tracing::error!(%error, "the repair source could not be read");
                return Err(RepairError::SourceFailed);
            }
        };
        if bytes.len() as u64 > self.max_bytes {
            return Err(RepairError::TooLarge {
                max: self.max_bytes,
            });
        }
        // The only check that matters, made here and not delegated to the
        // source: nothing that does not hash to the digest is ever written.
        if &ContentDigest::of_bytes(&bytes) != digest {
            return Err(RepairError::SourceMismatch);
        }

        let mut quarantined = false;
        if was == Was::Corrupt {
            let (Some(conditional), Some(marker)) = (self.store.conditional(), marker) else {
                return Err(RepairError::Unsupported);
            };
            let key = conditional.quarantine_key_of(digest, &format!("repair-{}", unique_suffix()));
            match self
                .io(conditional.quarantine_if_untouched(digest, &marker, &key))
                .await?
            {
                Ok(ConditionalOutcome::Applied { .. }) => quarantined = true,
                // Gone: someone removed it meanwhile; the key is free anyway.
                Ok(ConditionalOutcome::Gone) => {}
                Ok(ConditionalOutcome::NotApplied) => return Err(RepairError::Raced),
                Err(error) => {
                    tracing::error!(%error, "the damaged blob could not be quarantined");
                    return Err(RepairError::StoreFailed);
                }
            }
        }

        match self.io(self.store.put(&bytes)).await? {
            Ok(stored) if &stored == digest => {}
            Ok(_) => return Err(RepairError::StoreFailed),
            Err(error) => {
                tracing::error!(%error, "the verified bytes could not be written");
                return Err(RepairError::StoreFailed);
            }
        }
        // A repair that is not read back is a claim, not a fact.
        if !matches!(self.io(self.store.verify(digest)).await?, Ok(())) {
            return Err(RepairError::StoreFailed);
        }
        Ok(RepairOutcome::Repaired {
            was,
            bytes_written: bytes.len() as u64,
            quarantined,
            elapsed_ms: elapsed_ms(started.elapsed()),
        })
    }
}

fn elapsed_ms(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}
