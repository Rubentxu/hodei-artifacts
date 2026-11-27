//! Moving the reads of a blob between two stores, and retiring the origin copy
//! only when that is provably safe (HSP-325).
//!
//! [`crate::recovery::migrate`] copies. This module changes **where a digest is
//! read from** and lets the origin copy step aside, always through the location
//! journal (`hodei_catalog::location`): a row per digest with a lease and a
//! monotonic fence. The rules, none of them negotiable:
//!
//! - **the journal is the only switch.** A digest changes phase only by
//!   advancing its row under the live lease; a writer whose lease expired and
//!   was taken over holds an old fence and its advance is refused;
//! - **a switch needs a verified destination**: the bytes are read back from the
//!   destination and hashed here, never trusted from the store's word;
//! - **the origin is never deleted.** "Retiring" moves the exact version that
//!   was observed aside (the store's versioned quarantine), which is reversible:
//!   a rollback puts verified bytes back and flips the reads. Anything that
//!   cannot be proven (a store that cannot quarantine conditionally, a hold that
//!   cannot be read, a destination that does not verify, a digest nobody
//!   references) leaves the origin exactly as it was;
//! - **no hold of any scope** may reach the digest: a hold on the digest, on a
//!   repository that references it or on an account that does (the owners are
//!   read fresh from the catalog rows). The holds are read before the origin is
//!   touched and again after: a hold that appeared in between puts the origin
//!   copy back and leaves the reads where they were;
//! - **a stalled or fenced writer does no harm**: every advance is fenced, the
//!   lease is renewed right before the only side effect on a store, and the
//!   reads of a switched digest fall back to the other store if the first one
//!   does not hold verified bytes;
//! - the cost is bounded: digests per request, bytes per blob, one operation
//!   per account and [`MAX_CONCURRENT`] overall, a timeout on every call and a
//!   deadline on the whole request (what is not reached is `deferred`).

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, HoldScope, LocationEntry, LocationJournal,
    LocationLease, Phase,
};
use hodei_evidence::ContentDigest;
use hodei_hub::reclaim::ConditionalOutcome;
use hodei_hub::{BlobError, BlobStore};
use serde::{Deserialize, Serialize};

/// The most relocations that run at the same time.
pub const MAX_CONCURRENT: usize = 2;

/// The most blobs one request may name.
pub const MAX_BLOBS_PER_REQUEST: usize = 64;

/// How long a call to a store, the journal or the ledger may take by default.
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a whole request may hold its place by default.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(300);

/// How long the lease of one digest lasts by default.
pub const DEFAULT_LEASE: Duration = Duration::from_secs(90);

/// What is asked of a digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Make the destination the place reads go to (origin copy intact).
    Switch,
    /// Move the origin copy aside (reads already go to the destination).
    Retire,
    /// Make the origin the place reads go to again.
    Rollback,
}

impl Operation {
    /// The stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::Retire => "retire",
            Self::Rollback => "rollback",
        }
    }
}

/// What happened to one digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Relocated {
    /// Reads go to the destination now.
    Switched,
    /// The origin copy was moved aside (never deleted).
    Retired {
        /// The origin held no copy to move (it was already gone).
        origin_already_absent: bool,
    },
    /// Reads go to the origin again and the origin verifies.
    RolledBack,
    /// The digest was already where the operation leads: nothing changed.
    Unchanged,
    /// A retirement was asked for a digest whose reads still go to the origin.
    NotSwitched,
    /// The destination holds no copy.
    DestinationAbsent,
    /// The destination holds bytes that do not hash to the digest.
    DestinationCorrupt,
    /// The destination could not be read.
    DestinationFailed,
    /// The blob is above the ceiling of one blob.
    TooLarge {
        /// The ceiling.
        max: u64,
    },
    /// A hold reaches the digest: the origin was left (or put back) as it was.
    Held {
        /// The identifier of the hold, only when the hold belongs to the
        /// caller's own account (a repository or account hold of it). A hold of
        /// another account, or a digest hold (which names no account), is
        /// reported without its identifier: the digest is shared across
        /// accounts and the id must not leak.
        #[serde(skip_serializing_if = "Option::is_none")]
        hold: Option<String>,
    },
    /// A hold appeared while the origin copy was being moved aside **and the
    /// origin copy could not be put back**: the origin copy is still aside (the
    /// bytes are kept under a quarantine name), the phase is still `switched`
    /// and the destination serves. Nothing was lost, but the origin is NOT
    /// intact: run `rollback` once the store allows it.
    HeldOriginAside {
        /// The identifier of the hold, under the same rule as [`Self::Held`].
        #[serde(skip_serializing_if = "Option::is_none")]
        hold: Option<String>,
    },
    /// The holds could not be read after the origin copy was moved aside and
    /// it could not be put back: same state as [`Self::HeldOriginAside`].
    OriginAside,
    /// The holds could not be read, so nothing could be proven.
    HoldsUnreadable,
    /// No catalog row names the digest, so the holds cannot be assessed.
    NotReferenced,
    /// The origin store cannot move a version aside conditionally.
    Unsupported,
    /// The origin version changed while it was being retired: untouched.
    Raced,
    /// The origin could not be read or moved.
    OriginFailed,
    /// A rollback found no origin copy and could not restore one safely.
    NothingToRestore,
    /// A rollback found a damaged origin copy: repair it first.
    OriginCorrupt,
    /// Another writer holds the lease of the digest.
    Busy,
    /// The lease was lost (taken over) mid-operation: nothing else was done.
    Fenced,
    /// The journal failed.
    JournalFailed,
    /// The deadline of the request passed before this digest.
    Deferred,
}

impl Relocated {
    /// Whether the digest is where the operation leads, verifiably.
    pub fn is_done(&self) -> bool {
        matches!(
            self,
            Self::Switched | Self::Retired { .. } | Self::RolledBack | Self::Unchanged
        )
    }

    fn is_store_failure(&self) -> bool {
        matches!(
            self,
            Self::DestinationFailed
                | Self::HeldOriginAside { .. }
                | Self::OriginAside
                | Self::OriginFailed
                | Self::JournalFailed
                | Self::HoldsUnreadable
        )
    }
}

/// One entry of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The content, `sha256:<hex>`.
    pub digest: String,
    /// What happened to it.
    #[serde(flatten)]
    pub result: Relocated,
}

/// What a request did, digest by digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelocationReport {
    /// The operation.
    pub operation: Operation,
    /// One entry per digest, in the order asked and without duplicates.
    pub entries: Vec<Entry>,
    /// Digests that are where the operation leads.
    pub succeeded: usize,
    /// Digests that are not.
    pub pending: usize,
    /// Always `false`: nothing here deletes an origin copy.
    pub origin_deleted: bool,
    /// Whether the deadline of the request passed.
    pub deadline_exceeded: bool,
    /// How long it took, in milliseconds.
    pub elapsed_ms: u64,
}

impl RelocationReport {
    /// Whether any entry failed because a store, the journal or the holds could
    /// not be read.
    pub fn any_store_failure(&self) -> bool {
        self.entries.iter().any(|e| e.result.is_store_failure())
    }
}

/// Why a request did not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelocateError {
    /// More digests than one request may name.
    #[error("a request may name at most {max} digests")]
    TooMany {
        /// The ceiling.
        max: usize,
    },
    /// The places are taken, or this account already has one.
    #[error("too many relocations are running")]
    Busy,
    /// The task itself failed.
    #[error("the relocation failed")]
    Failed,
}

/// The place of one request: a global one and the only one its account may
/// hold. Both come back when it is dropped.
pub struct Reservation {
    _permit: tokio::sync::OwnedSemaphorePermit,
    accounts: Arc<Mutex<HashSet<String>>>,
    account: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut accounts) = self.accounts.lock() {
            accounts.remove(&self.account);
        }
    }
}

/// The relocation of blobs between the origin and the destination.
pub struct Relocator {
    origin: Arc<dyn BlobStore>,
    destination: Arc<dyn BlobStore>,
    journal: Arc<dyn LocationJournal>,
    ledger: Arc<dyn ContentLedger>,
    max_blob_bytes: u64,
    io_timeout: Duration,
    deadline: Duration,
    lease: Duration,
    permits: Arc<tokio::sync::Semaphore>,
    accounts: Arc<Mutex<HashSet<String>>>,
}

impl Relocator {
    /// A relocator between `origin` and `destination` that never reads a blob
    /// above `max_blob_bytes`.
    pub fn new(
        origin: Arc<dyn BlobStore>,
        destination: Arc<dyn BlobStore>,
        journal: Arc<dyn LocationJournal>,
        ledger: Arc<dyn ContentLedger>,
        max_blob_bytes: u64,
    ) -> Self {
        Self {
            origin,
            destination,
            journal,
            ledger,
            max_blob_bytes,
            io_timeout: DEFAULT_IO_TIMEOUT,
            deadline: DEFAULT_DEADLINE,
            lease: DEFAULT_LEASE,
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
            accounts: Arc::default(),
        }
    }

    /// Another timeout for each call.
    pub fn with_io_timeout(mut self, io_timeout: Duration) -> Self {
        self.io_timeout = io_timeout;
        self
    }

    /// Another deadline for a whole request.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Another lease for each digest.
    pub fn with_lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// Takes a place for `account`, or says there is none.
    pub fn reserve(&self, account: &str) -> Result<Reservation, RelocateError> {
        let mut accounts = self.accounts.lock().map_err(|_| RelocateError::Failed)?;
        if accounts.contains(account) {
            return Err(RelocateError::Busy);
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| RelocateError::Busy)?;
        accounts.insert(account.to_string());
        Ok(Reservation {
            _permit: permit,
            accounts: self.accounts.clone(),
            account: account.to_string(),
        })
    }

    /// Runs `operation` over `digests` (duplicates dropped, order kept) under a
    /// place taken with [`Self::reserve`]. The caller runs this in a task of its
    /// own, so a client that hangs up leaves neither the work half reported nor
    /// the place taken.
    pub async fn relocate(
        &self,
        place: &Reservation,
        operation: Operation,
        digests: &[ContentDigest],
    ) -> Result<RelocationReport, RelocateError> {
        let mut unique: Vec<ContentDigest> = Vec::with_capacity(digests.len());
        for digest in digests {
            if !unique.contains(digest) {
                unique.push(digest.clone());
            }
        }
        if unique.len() > MAX_BLOBS_PER_REQUEST {
            return Err(RelocateError::TooMany {
                max: MAX_BLOBS_PER_REQUEST,
            });
        }
        let started = Instant::now();
        let deadline = started + self.deadline;
        let mut exceeded = false;
        let mut entries = Vec::with_capacity(unique.len());
        for digest in &unique {
            exceeded = exceeded || Instant::now() >= deadline;
            let result = if exceeded {
                Relocated::Deferred
            } else {
                self.one(operation, digest, &place.account, deadline).await
            };
            entries.push(Entry {
                digest: digest.to_string(),
                result,
            });
        }
        let succeeded = entries.iter().filter(|e| e.result.is_done()).count();
        Ok(RelocationReport {
            operation,
            pending: entries.len() - succeeded,
            succeeded,
            entries,
            origin_deleted: false,
            deadline_exceeded: exceeded,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// One call, bounded in time: a call that times out is a failure.
    async fn io<T, E>(
        &self,
        deadline: Instant,
        call: impl Future<Output = Result<T, E>>,
        timed_out: E,
    ) -> Result<T, E> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::timeout(self.io_timeout.min(remaining), call)
            .await
            .unwrap_or(Err(timed_out))
    }

    async fn blob<T>(
        &self,
        deadline: Instant,
        call: impl Future<Output = Result<T, BlobError>>,
    ) -> Result<T, BlobError> {
        self.io(
            deadline,
            call,
            BlobError::Unavailable("the call timed out".to_string()),
        )
        .await
    }

    async fn catalog<T>(
        &self,
        deadline: Instant,
        call: impl Future<Output = Result<T, CatalogError>>,
    ) -> Result<T, CatalogError> {
        self.io(
            deadline,
            call,
            CatalogError::unavailable("the call timed out"),
        )
        .await
    }

    async fn one(
        &self,
        operation: Operation,
        digest: &ContentDigest,
        account: &str,
        deadline: Instant,
    ) -> Relocated {
        // One owner per operation: two tasks of this process never share a
        // lease by accident (the same owner would renew, not exclude).
        let owner = format!("relocate-{}", uuid::Uuid::new_v4().simple());
        let mut lease = match self
            .catalog(
                deadline,
                self.journal
                    .acquire_location(digest, &owner, self.lease, Utc::now()),
            )
            .await
        {
            Ok(lease) => lease,
            Err(CatalogError::Conflict { .. }) => return Relocated::Busy,
            Err(error) => {
                tracing::error!(%error, "the location lease could not be taken");
                return Relocated::JournalFailed;
            }
        };
        let result = self
            .guarded(operation, digest, account, &owner, &mut lease, deadline)
            .await;
        // Best effort: an unreleased lease just expires.
        let _ = self
            .catalog(deadline, self.journal.release_location(&lease))
            .await;
        result
    }

    async fn guarded(
        &self,
        operation: Operation,
        digest: &ContentDigest,
        account: &str,
        owner: &str,
        lease: &mut LocationLease,
        deadline: Instant,
    ) -> Relocated {
        let entry = match self
            .catalog(deadline, self.journal.location_of(digest))
            .await
        {
            Ok(Some(entry)) => entry,
            Ok(None) => return Relocated::JournalFailed,
            Err(error) => {
                tracing::error!(%error, "the location could not be read");
                return Relocated::JournalFailed;
            }
        };
        match operation {
            Operation::Switch => self.switch(digest, &entry, lease, deadline).await,
            Operation::Retire => {
                self.retire(digest, &entry, account, owner, lease, deadline)
                    .await
            }
            Operation::Rollback => self.rollback(digest, &entry, lease, deadline).await,
        }
    }

    /// The bytes of `store`, read back and hashed here. `Err` is the outcome
    /// that says why they cannot be trusted.
    async fn verified(
        &self,
        store: &Arc<dyn BlobStore>,
        digest: &ContentDigest,
        deadline: Instant,
        absent: Relocated,
        corrupt: Relocated,
        failed: Relocated,
    ) -> Result<Vec<u8>, Relocated> {
        match self.blob(deadline, store.length(digest)).await {
            Ok(length) if length > self.max_blob_bytes => {
                return Err(Relocated::TooLarge {
                    max: self.max_blob_bytes,
                });
            }
            Ok(_) => {}
            Err(BlobError::NotFound) => return Err(absent),
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Err(corrupt);
            }
            Err(error) => {
                tracing::error!(%error, "a store could not be measured");
                return Err(failed);
            }
        }
        let bytes = match self.blob(deadline, store.get(digest)).await {
            Ok(bytes) => bytes,
            Err(BlobError::NotFound) => return Err(absent),
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Err(corrupt);
            }
            Err(error) => {
                tracing::error!(%error, "a store could not be read");
                return Err(failed);
            }
        };
        // The only check that matters, made here and not delegated.
        if bytes.len() as u64 > self.max_blob_bytes || &ContentDigest::of_bytes(&bytes) != digest {
            return Err(corrupt);
        }
        Ok(bytes)
    }

    async fn destination_bytes(
        &self,
        digest: &ContentDigest,
        deadline: Instant,
    ) -> Result<Vec<u8>, Relocated> {
        self.verified(
            &self.destination,
            digest,
            deadline,
            Relocated::DestinationAbsent,
            Relocated::DestinationCorrupt,
            Relocated::DestinationFailed,
        )
        .await
    }

    async fn advance(
        &self,
        lease: &LocationLease,
        from: Phase,
        to: Phase,
        deadline: Instant,
    ) -> Result<(), Relocated> {
        match self
            .catalog(deadline, self.journal.advance_location(lease, from, to))
            .await
        {
            Ok(_) => Ok(()),
            Err(CatalogError::PreconditionFailed { .. } | CatalogError::Conflict { .. }) => {
                Err(Relocated::Fenced)
            }
            Err(error) => {
                tracing::error!(%error, "the location could not be advanced");
                Err(Relocated::JournalFailed)
            }
        }
    }

    async fn switch(
        &self,
        digest: &ContentDigest,
        entry: &LocationEntry,
        lease: &LocationLease,
        deadline: Instant,
    ) -> Relocated {
        if entry.phase != Phase::Origin {
            return Relocated::Unchanged;
        }
        if let Err(outcome) = self.destination_bytes(digest, deadline).await {
            return outcome;
        }
        match self
            .advance(lease, Phase::Origin, Phase::Switched, deadline)
            .await
        {
            Ok(()) => Relocated::Switched,
            Err(outcome) => outcome,
        }
    }

    /// The first hold in force that reaches the digest, if any.
    async fn covering_hold(
        &self,
        digest: &ContentDigest,
        deadline: Instant,
    ) -> Result<Option<ContentHold>, Relocated> {
        let owners = self
            .catalog(deadline, self.journal.digest_owners(digest))
            .await
            .map_err(|error| {
                tracing::error!(%error, "the owners of the digest could not be read");
                Relocated::HoldsUnreadable
            })?;
        if owners.is_empty() {
            return Err(Relocated::NotReferenced);
        }
        let holds = self
            .catalog(deadline, self.ledger.holds())
            .await
            .map_err(|error| {
                tracing::error!(%error, "the holds could not be read");
                Relocated::HoldsUnreadable
            })?;
        let now = Utc::now();
        Ok(holds.into_iter().find(|hold| {
            hold.is_active(now)
                && match &hold.scope {
                    HoldScope::Digest { digest: held } => held == digest,
                    HoldScope::Repository {
                        account,
                        repository,
                    } => owners.iter().any(|(a, r)| a == account && r == repository),
                    HoldScope::Account { account } => owners.iter().any(|(a, _)| a == account),
                }
        }))
    }

    async fn retire(
        &self,
        digest: &ContentDigest,
        entry: &LocationEntry,
        account: &str,
        owner: &str,
        lease: &mut LocationLease,
        deadline: Instant,
    ) -> Relocated {
        match entry.phase {
            Phase::Origin => return Relocated::NotSwitched,
            Phase::Retired => return Relocated::Unchanged,
            Phase::Switched => {}
        }
        match self.covering_hold(digest, deadline).await {
            Ok(None) => {}
            Ok(Some(hold)) => {
                return Relocated::Held {
                    hold: visible_id(&hold, account),
                };
            }
            Err(outcome) => return outcome,
        }
        let bytes = match self.destination_bytes(digest, deadline).await {
            Ok(bytes) => bytes,
            Err(outcome) => return outcome,
        };
        let Some(conditional) = self
            .origin
            .conditional()
            .filter(|c| c.supports_conditional_delete())
        else {
            return Relocated::Unsupported;
        };
        let marker = match self.blob(deadline, conditional.head_marker(digest)).await {
            Ok(marker) => marker,
            Err(error) => {
                tracing::error!(%error, "the origin version could not be read");
                return Relocated::OriginFailed;
            }
        };
        let mut already_absent = marker.is_none();
        if let Some(marker) = marker {
            // The lease is renewed right before the only side effect on a
            // store: a gap in the claim (a take-over) stops the operation.
            match self
                .catalog(
                    deadline,
                    self.journal
                        .acquire_location(digest, owner, self.lease, Utc::now()),
                )
                .await
            {
                Ok(renewed) if renewed.fence == lease.fence && renewed.token == lease.token => {
                    *lease = renewed;
                }
                Ok(_) | Err(CatalogError::Conflict { .. }) => return Relocated::Fenced,
                Err(error) => {
                    tracing::error!(%error, "the lease could not be renewed");
                    return Relocated::JournalFailed;
                }
            }
            let key = conditional.quarantine_key_of(digest, &format!("relocate-{}", lease.fence));
            match self
                .blob(
                    deadline,
                    conditional.quarantine_if_untouched(digest, &marker, &key),
                )
                .await
            {
                Ok(ConditionalOutcome::Applied { .. }) => {
                    // A hold placed while the origin was being moved: put the
                    // origin back and leave the reads where they were.
                    match self.covering_hold(digest, deadline).await {
                        Ok(None) => {}
                        other => {
                            let restored = self.restore_origin(digest, &bytes, deadline).await;
                            return match (other, restored) {
                                (Ok(Some(hold)), true) => Relocated::Held {
                                    hold: visible_id(&hold, account),
                                },
                                (Ok(Some(hold)), false) => Relocated::HeldOriginAside {
                                    hold: visible_id(&hold, account),
                                },
                                (Err(outcome), true) => outcome,
                                (Err(_) | Ok(None), false) => Relocated::OriginAside,
                                (Ok(None), true) => Relocated::HoldsUnreadable,
                            };
                        }
                    }
                }
                Ok(ConditionalOutcome::Gone) => already_absent = true,
                Ok(ConditionalOutcome::NotApplied) => return Relocated::Raced,
                Err(error) => {
                    tracing::error!(%error, "the origin copy could not be moved aside");
                    return Relocated::OriginFailed;
                }
            }
        }
        match self
            .advance(lease, Phase::Switched, Phase::Retired, deadline)
            .await
        {
            Ok(()) => Relocated::Retired {
                origin_already_absent: already_absent,
            },
            Err(outcome) => outcome,
        }
    }

    /// Puts verified bytes back in the origin. Failure is logged, not hidden:
    /// the journal still reads from the destination, so no read is lost.
    async fn restore_origin(
        &self,
        digest: &ContentDigest,
        bytes: &[u8],
        deadline: Instant,
    ) -> bool {
        let stored = self.blob(deadline, self.origin.put(bytes)).await;
        let ok = matches!(&stored, Ok(d) if d == digest)
            && self
                .blob(deadline, self.origin.verify(digest))
                .await
                .is_ok();
        if !ok {
            tracing::error!(%digest, "the origin copy could not be restored");
        }
        ok
    }

    async fn rollback(
        &self,
        digest: &ContentDigest,
        entry: &LocationEntry,
        lease: &LocationLease,
        deadline: Instant,
    ) -> Relocated {
        if entry.phase == Phase::Origin {
            return Relocated::Unchanged;
        }
        // Reads only go back to an origin that verifies now.
        match self.blob(deadline, self.origin.verify(digest)).await {
            Ok(()) => {}
            Err(BlobError::Corrupted { .. } | BlobError::LengthMismatch { .. }) => {
                return Relocated::OriginCorrupt;
            }
            Err(BlobError::NotFound) => {
                let bytes = match self.destination_bytes(digest, deadline).await {
                    Ok(bytes) => bytes,
                    Err(Relocated::DestinationAbsent | Relocated::DestinationCorrupt) => {
                        return Relocated::NothingToRestore;
                    }
                    Err(outcome) => return outcome,
                };
                if !self.restore_origin(digest, &bytes, deadline).await {
                    return Relocated::OriginFailed;
                }
            }
            Err(error) => {
                tracing::error!(%error, "the origin could not be checked");
                return Relocated::OriginFailed;
            }
        }
        match self
            .advance(lease, entry.phase, Phase::Origin, deadline)
            .await
        {
            Ok(()) => Relocated::RolledBack,
            Err(outcome) => outcome,
        }
    }
}

/// The identifier of a hold, only if it is the caller's own: a repository or
/// account hold of the caller's account. Holds of other accounts, and digest
/// holds (which name no account), never leak their identifier.
fn visible_id(hold: &ContentHold, account: &str) -> Option<String> {
    let own = match &hold.scope {
        HoldScope::Repository { account: a, .. } | HoldScope::Account { account: a } => {
            a == account
        }
        HoldScope::Digest { .. } => false,
    };
    own.then(|| hold.id.clone())
}
