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
//! - one repair at a time, with a ceiling on the bytes a repair may move;
//! - repairing twice is the same as repairing once: the second finds a sound
//!   blob and does nothing.
//!
//! Any failure leaves the answer "not repaired" and says why. It never invents
//! bytes and never reports a repair it did not verify afterwards.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_evidence::ContentDigest;
use hodei_hub::reclaim::ConditionalOutcome;
use hodei_hub::{BlobError, BlobStore};
use serde::Serialize;

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
    /// The store failed, or the repaired blob did not verify afterwards.
    #[error("the content store failed during the repair")]
    StoreFailed,
}

/// The repair of one blob.
pub struct Repairer {
    store: Arc<dyn BlobStore>,
    source: Arc<dyn BlobStore>,
    max_bytes: u64,
    /// One repair at a time: the second request of a pair finds the blob
    /// already sound instead of racing the first.
    running: tokio::sync::Mutex<()>,
}

impl Repairer {
    /// A repairer of `store` that takes bytes only from `source`, never more
    /// than `max_bytes` for one blob.
    pub fn new(store: Arc<dyn BlobStore>, source: Arc<dyn BlobStore>, max_bytes: u64) -> Self {
        Self {
            store,
            source,
            max_bytes,
            running: tokio::sync::Mutex::new(()),
        }
    }

    /// The ceiling on the bytes of one repair.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Repairs the blob named by `digest`, if it needs it.
    pub async fn repair(&self, digest: &ContentDigest) -> Result<RepairOutcome, RepairError> {
        let _one_at_a_time = self.running.lock().await;
        let started = Instant::now();

        // The version that is observed is pinned *before* it is judged, so the
        // version judged damaged is the only one that can be moved away.
        let marker = match self.store.conditional() {
            Some(conditional) if conditional.supports_conditional_delete() => {
                match conditional.head_marker(digest).await {
                    Ok(marker) => Some(marker),
                    Err(error) => {
                        tracing::error!(%error, "the repair could not read the blob version");
                        return Err(RepairError::StoreFailed);
                    }
                }
            }
            _ => None,
        };

        let was = match self.store.verify(digest).await {
            Ok(()) => return Ok(RepairOutcome::AlreadySound),
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => Was::Corrupt,
            Err(BlobError::NotFound) => Was::Absent,
            Err(error) => {
                tracing::error!(%error, "the repair could not judge the blob");
                return Err(RepairError::StoreFailed);
            }
        };

        // The ceiling is checked before any byte is moved.
        match self.source.length(digest).await {
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
        let bytes = match self.source.get(digest).await {
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
            let (Some(conditional), Some(marker)) = (self.store.conditional(), marker.flatten())
            else {
                return Err(RepairError::Unsupported);
            };
            let key = conditional.quarantine_key_of(digest, &format!("repair-{}", unique_suffix()));
            match conditional
                .quarantine_if_untouched(digest, &marker, &key)
                .await
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

        match self.store.put(&bytes).await {
            Ok(stored) if &stored == digest => {}
            Ok(_) => return Err(RepairError::StoreFailed),
            Err(error) => {
                tracing::error!(%error, "the verified bytes could not be written");
                return Err(RepairError::StoreFailed);
            }
        }
        // A repair that is not read back is a claim, not a fact.
        if self.store.verify(digest).await.is_err() {
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
