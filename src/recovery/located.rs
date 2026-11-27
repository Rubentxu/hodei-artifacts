//! The content store whose reads follow the location journal (HSP-325).
//!
//! Reads of a digest go where its journal row says (see
//! `hodei_catalog::location`):
//!
//! - no row, or phase `origin`: the origin, and only the origin;
//! - phase `switched` or `retired`: the destination first and, if it does not
//!   hold verified bytes, the origin (every store verifies the digest it
//!   returns, so a fallback can never serve other bytes);
//! - the journal cannot be read (error or timeout; bounded by a short timeout and
//!   a breaker that skips the journal for a few seconds): the origin and then the
//!   destination, for the
//!   same reason. A read never answers with unverified bytes.
//!
//! Writes always go to the origin (a new blob is born there), and the reclaim
//! and conditional views are the origin's: the garbage collection and the repair
//! keep working on the origin and are neither blocked nor redirected by the
//! journal.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hodei_catalog::{CatalogError, LocationEntry, LocationJournal, LocationLease, Phase};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobWriteSession, ReadPolicy};
use hodei_hub::reclaim::{BlobReclaim, ConditionalBlob};
use hodei_hub::{BlobError, BlobStore};

/// How long a read waits for the journal by default.
pub const DEFAULT_JOURNAL_TIMEOUT: Duration = Duration::from_secs(1);

/// How long reads skip the journal after it failed or timed out, by default.
pub const DEFAULT_JOURNAL_COOLDOWN: Duration = Duration::from_secs(5);

/// The accepted range of the journal timeout.
pub const JOURNAL_TIMEOUT_RANGE: std::ops::RangeInclusive<Duration> =
    Duration::from_millis(100)..=Duration::from_secs(10);

/// The accepted range of the breaker cooldown.
pub const JOURNAL_COOLDOWN_RANGE: std::ops::RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_secs(60);

/// A journal that is plugged in after the store that needs it is built (the
/// catalog that holds the journal is opened with a probe over this very store).
/// Until it is plugged in every call fails, and the reads fall back to the
/// origin and then the destination.
#[derive(Default)]
pub struct DeferredJournal {
    inner: std::sync::OnceLock<Arc<dyn LocationJournal>>,
}

impl DeferredJournal {
    /// Plugs the real journal in (the first call wins).
    pub fn plug(&self, journal: Arc<dyn LocationJournal>) {
        let _ = self.inner.set(journal);
    }

    fn journal(&self) -> Result<&Arc<dyn LocationJournal>, CatalogError> {
        self.inner
            .get()
            .ok_or_else(|| CatalogError::unavailable("the location journal is not plugged in"))
    }
}

#[async_trait]
impl LocationJournal for DeferredJournal {
    async fn location_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<LocationEntry>, CatalogError> {
        self.journal()?.location_of(digest).await
    }

    async fn acquire_location(
        &self,
        digest: &ContentDigest,
        owner: &str,
        lease_for: Duration,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<LocationLease, CatalogError> {
        self.journal()?
            .acquire_location(digest, owner, lease_for, now)
            .await
    }

    async fn advance_location(
        &self,
        lease: &LocationLease,
        from: Phase,
        to: Phase,
    ) -> Result<LocationEntry, CatalogError> {
        self.journal()?.advance_location(lease, from, to).await
    }

    async fn release_location(&self, lease: &LocationLease) -> Result<(), CatalogError> {
        self.journal()?.release_location(lease).await
    }

    async fn digest_owners(
        &self,
        digest: &ContentDigest,
    ) -> Result<Vec<(String, String)>, CatalogError> {
        self.journal()?.digest_owners(digest).await
    }
}

/// A content store that reads where the journal says.
pub struct LocatedStore {
    origin: Arc<dyn BlobStore>,
    destination: Arc<dyn BlobStore>,
    journal: Arc<dyn LocationJournal>,
    journal_timeout: Duration,
    cooldown: Duration,
    /// While set and in the future, reads do not ask the journal (a breaker: a
    /// hung journal costs one timeout per cooldown, not one per read).
    open_until: std::sync::Mutex<Option<std::time::Instant>>,
}

impl LocatedStore {
    /// A store over `origin` and `destination`, following `journal`.
    pub fn new(
        origin: Arc<dyn BlobStore>,
        destination: Arc<dyn BlobStore>,
        journal: Arc<dyn LocationJournal>,
    ) -> Self {
        Self {
            origin,
            destination,
            journal,
            journal_timeout: DEFAULT_JOURNAL_TIMEOUT,
            cooldown: DEFAULT_JOURNAL_COOLDOWN,
            open_until: std::sync::Mutex::new(None),
        }
    }

    /// The same store with another journal timeout and breaker cooldown. Values
    /// outside [`JOURNAL_TIMEOUT_RANGE`] / [`JOURNAL_COOLDOWN_RANGE`] are
    /// refused, never clamped silently.
    pub fn with_journal_limits(
        mut self,
        timeout: Duration,
        cooldown: Duration,
    ) -> Result<Self, &'static str> {
        if !JOURNAL_TIMEOUT_RANGE.contains(&timeout) {
            return Err("the journal timeout must be between 100 ms and 10 s");
        }
        if !JOURNAL_COOLDOWN_RANGE.contains(&cooldown) {
            return Err("the journal cooldown must be between 1 s and 60 s");
        }
        self.journal_timeout = timeout;
        self.cooldown = cooldown;
        Ok(self)
    }

    fn breaker_open(&self) -> bool {
        self.open_until
            .lock()
            .map(|until| until.is_some_and(|at| std::time::Instant::now() < at))
            .unwrap_or(true)
    }

    fn trip(&self) {
        if let Ok(mut until) = self.open_until.lock() {
            *until = Some(std::time::Instant::now() + self.cooldown);
        }
    }

    /// The stores a read of `digest` may use, first choice first.
    async fn order(&self, digest: &ContentDigest) -> Vec<&Arc<dyn BlobStore>> {
        if self.breaker_open() {
            return vec![&self.origin, &self.destination];
        }
        match tokio::time::timeout(self.journal_timeout, self.journal.location_of(digest)).await {
            Ok(Ok(Some(entry))) if entry.phase.reads_destination() => {
                vec![&self.destination, &self.origin]
            }
            Ok(Ok(Some(entry))) if entry.phase == Phase::Origin => vec![&self.origin],
            Ok(Ok(None)) => vec![&self.origin],
            Ok(Ok(Some(_))) => vec![&self.origin],
            Ok(Err(error)) => {
                self.trip();
                tracing::error!(%error, "the location journal could not be read");
                vec![&self.origin, &self.destination]
            }
            Err(_) => {
                self.trip();
                tracing::error!("the location journal timed out");
                vec![&self.origin, &self.destination]
            }
        }
    }
}

/// Whether the next store may be tried: the first holds nothing it can vouch for.
fn may_fall_back(error: &BlobError) -> bool {
    matches!(
        error,
        BlobError::NotFound
            | BlobError::Corrupted { .. }
            | BlobError::LengthMismatch { .. }
            | BlobError::Unavailable(_)
    )
}

macro_rules! first_that_answers {
    ($self:ident, $digest:expr, |$store:ident| $call:expr) => {{
        let mut first_error = None;
        let mut answer = None;
        for $store in $self.order($digest).await {
            match $call.await {
                Ok(value) => {
                    answer = Some(value);
                    break;
                }
                Err(error) => {
                    let again = may_fall_back(&error);
                    first_error.get_or_insert(error);
                    if !again {
                        break;
                    }
                }
            }
        }
        match answer {
            Some(value) => Ok(value),
            None => Err(first_error.unwrap_or(BlobError::NotFound)),
        }
    }};
}

#[async_trait]
impl BlobStore for LocatedStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.origin.put(bytes).await
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        first_that_answers!(self, digest, |store| store.get(digest))
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        // "Not there" in the first store is an answer only if it is the last.
        let mut first_error = None;
        let stores = self.order(digest).await;
        let last = stores.len() - 1;
        for (index, store) in stores.into_iter().enumerate() {
            match store.exists(digest).await {
                Ok(true) => return Ok(true),
                Ok(false) if index == last => return Ok(false),
                Ok(false) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        Err(first_error.unwrap_or(BlobError::NotFound))
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        self.origin.begin().await
    }

    async fn resume(
        &self,
        handle: &str,
        written: u64,
    ) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        self.origin.resume(handle, written).await
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        first_that_answers!(self, digest, |store| store.get_with(digest, policy))
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        first_that_answers!(self, digest, |store| store.length(digest))
    }

    async fn age(&self, digest: &ContentDigest) -> Result<Option<std::time::Duration>, BlobError> {
        first_that_answers!(self, digest, |store| store.age(digest))
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        first_that_answers!(self, digest, |store| store
            .read_range(digest, offset, length))
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        first_that_answers!(self, digest, |store| store.verify(digest))
    }

    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.origin.reclaimer()
    }

    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        self.origin.conditional()
    }
}
