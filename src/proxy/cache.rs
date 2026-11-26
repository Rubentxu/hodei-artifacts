//! The proxy cache's state pieces: download coalescing,
//! negative cache, metrics and the error shared between requests.
//!
//! The cache **decision** (freshness, revalidation, offline, quarantine) is
//! pure and lives in `hodei_catalog::proxy_cache`. Only what needs process
//! memory is here:
//!
//! - [`SingleFlight`]: concurrent requests for the same path share
//!   a single download. One download per path, within the global limit on
//!   simultaneous egress already enforced by the egress client.
//! - [`NegativeCache`]: a path the upstream said does not exist is not
//!   asked about again for a short time.
//! - [`ProxyMetrics`]: counters of what happened with each request.
//!
//! None of this is a source of truth: if the process restarts, all that is
//! lost is an optimization. Freshness and provenance always come from the
//! receipt ledger.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::watch;

/// The role a request played in a shared download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flight<V> {
    /// This request did the work.
    Led(V),
    /// This request waited and shares the result of another.
    Followed(V),
}

impl<V> Flight<V> {
    /// The result, whatever the role.
    pub fn into_inner(self) -> V {
        match self {
            Self::Led(value) | Self::Followed(value) => value,
        }
    }

    /// Whether this request shared the work of another.
    pub fn is_followed(&self) -> bool {
        matches!(self, Self::Followed(_))
    }
}

/// Coalescing: a single in-flight execution per key.
///
/// The first to arrive (the *leader*) runs the work; those arriving
/// while it runs wait and receive a copy of its result, success or
/// error. If the leader is cancelled (the client closes the connection),
/// followers are not left hanging nor do they inherit the cancellation: one
/// of them becomes the new leader.
///
/// The shared result is the leader's, including on whose behalf the receipt
/// was recorded: everyone who waited was already allowed to read that
/// path, because authorization happens before getting here.
pub struct SingleFlight<K, V> {
    inflight: Mutex<HashMap<K, watch::Receiver<Option<V>>>>,
}

impl<K, V> Default for SingleFlight<K, V> {
    fn default() -> Self {
        Self {
            inflight: Mutex::new(HashMap::new()),
        }
    }
}

impl<K, V> std::fmt::Debug for SingleFlight<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleFlight").finish_non_exhaustive()
    }
}

/// Removes the entry if the leader finishes without publishing (cancellation or panic).
struct LeaderGuard<'a, K: Eq + Hash, V> {
    flights: &'a SingleFlight<K, V>,
    key: Option<K>,
}

impl<K: Eq + Hash, V> LeaderGuard<'_, K, V> {
    /// Removes the entry and makes the `Drop` a no-op.
    fn release(&mut self) {
        if let Some(key) = self.key.take() {
            self.flights.remove(&key);
        }
    }
}

impl<K: Eq + Hash, V> Drop for LeaderGuard<'_, K, V> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<K: Eq + Hash, V> SingleFlight<K, V> {
    fn remove(&self, key: &K) {
        // A poisoned mutex can only come from a panic while holding the lock;
        // the map is still valid.
        let mut inflight = self
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inflight.remove(key);
    }

    /// How many executions are in flight.
    pub fn in_flight(&self) -> usize {
        self.inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl<K: Eq + Hash + Clone, V: Clone> SingleFlight<K, V> {
    /// An empty coalescer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `work` if nobody is doing it for `key`; otherwise waits for
    /// whoever is doing it and returns its result.
    pub async fn run<F, Fut>(&self, key: K, work: F) -> Flight<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = V>,
    {
        let mut work = Some(work);
        loop {
            enum Role<V> {
                Leader(watch::Sender<Option<V>>),
                Follower(watch::Receiver<Option<V>>),
            }
            let role = {
                let mut inflight = self
                    .inflight
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match inflight.get(&key) {
                    Some(receiver) => Role::Follower(receiver.clone()),
                    None => {
                        let (sender, receiver) = watch::channel(None);
                        inflight.insert(key.clone(), receiver);
                        Role::Leader(sender)
                    }
                }
            };
            match role {
                Role::Leader(sender) => {
                    let mut guard = LeaderGuard {
                        flights: self,
                        key: Some(key.clone()),
                    };
                    // A leader ends the function: the work is requested only once.
                    let Some(work) = work.take() else {
                        continue;
                    };
                    let value = work().await;
                    // The entry is removed first: whoever arrives now opens
                    // a new download instead of reading one that already finished.
                    guard.release();
                    // Without followers there is nobody to notify and `send`
                    // fails: that is not an error.
                    let _ = sender.send(Some(value.clone()));
                    return Flight::Led(value);
                }
                Role::Follower(mut receiver) => {
                    // If the leader was cancelled with no result, `wait_for`
                    // fails and it retries: whoever arrives first takes over.
                    if let Ok(value) = receiver.wait_for(|value| value.is_some()).await
                        && let Some(value) = value.clone()
                    {
                        return Flight::Followed(value);
                    }
                }
            }
        }
    }
}

/// The key of a download or an absence: account, repository and path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    /// The owning account.
    pub account: String,
    /// The repository key.
    pub repository: String,
    /// The path.
    pub asset_path: String,
}

impl PathKey {
    /// The key of a path in a repository of an account.
    pub fn of(account: &str, repository: &str, asset_path: &str) -> Self {
        Self {
            account: account.to_string(),
            repository: repository.to_string(),
            asset_path: asset_path.to_string(),
        }
    }
}

/// How many absences the negative cache remembers at most.
const NEGATIVE_CAPACITY: usize = 4096;

/// The cache of what the upstream said does not exist.
///
/// In memory and bounded. The key carries the repository's **configuration
/// version**: changing the upstream, the scopes or the policy effectively
/// discards all earlier absences, because the question is no longer the
/// same. It only remembers absences for paths **without content**: a path
/// the catalog already serves is never hidden behind an absence.
#[derive(Debug, Default)]
pub struct NegativeCache {
    entries: Mutex<HashMap<(PathKey, u64), Instant>>,
}

impl NegativeCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether there is a live absence for that path and configuration version.
    pub fn contains(&self, key: &PathKey, config_version: u64, now: Instant) -> bool {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries
            .get(&(key.clone(), config_version))
            .is_some_and(|expires| *expires > now)
    }

    /// Remembers an absence for `ttl`. With the cache full (after purging
    /// expired entries) it stores nothing: it is an optimization, not a record.
    pub fn remember(&self, key: &PathKey, config_version: u64, ttl: Duration, now: Instant) {
        if ttl.is_zero() {
            return;
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= NEGATIVE_CAPACITY {
            entries.retain(|_, expires| *expires > now);
            if entries.len() >= NEGATIVE_CAPACITY {
                return;
            }
        }
        entries.insert((key.clone(), config_version), now + ttl);
    }

    /// Forgets the absence of a path, whatever the version.
    pub fn forget(&self, key: &PathKey) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|(stored, _), _| stored != key);
    }

    /// How many absences are stored (expired ones included).
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The proxy counters. Atomic and unlabeled: they are a pulse of the
/// process, not a time series; the trace (`tracing`) carries the detail.
#[derive(Debug, Default)]
pub struct ProxyMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    revalidated: AtomicU64,
    refreshed: AtomicU64,
    stale: AtomicU64,
    negative_hits: AtomicU64,
    offline_hits: AtomicU64,
    offline_misses: AtomicU64,
    quarantined: AtomicU64,
    coalesced: AtomicU64,
    upstream_requests: AtomicU64,
    upstream_errors: AtomicU64,
}

/// A reading of the counters at an instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// Served from the cache without going out.
    pub hits: u64,
    /// Was missing and was fetched.
    pub misses: u64,
    /// Was expired and the upstream confirmed it.
    pub revalidated: u64,
    /// Was expired and the upstream delivered new bytes.
    pub refreshed: u64,
    /// The upstream failed and what was there was served, within the window.
    pub stale: u64,
    /// A remembered absence saved a question.
    pub negative_hits: u64,
    /// Served in offline mode.
    pub offline_hits: u64,
    /// Rejected in offline mode for not being in the cache.
    pub offline_misses: u64,
    /// Held back for delivering bytes different from those of immutable content.
    pub quarantined: u64,
    /// Requests that shared another's download.
    pub coalesced: u64,
    /// Egress calls to the upstream.
    pub upstream_requests: u64,
    /// Egress calls to the upstream that failed.
    pub upstream_errors: u64,
}

macro_rules! counter {
    ($record:ident, $field:ident) => {
        /// Adds one.
        pub fn $record(&self) {
            self.$field.fetch_add(1, Ordering::Relaxed);
        }
    };
}

impl ProxyMetrics {
    counter!(record_hit, hits);
    counter!(record_miss, misses);
    counter!(record_revalidated, revalidated);
    counter!(record_refreshed, refreshed);
    counter!(record_stale, stale);
    counter!(record_negative_hit, negative_hits);
    counter!(record_offline_hit, offline_hits);
    counter!(record_offline_miss, offline_misses);
    counter!(record_quarantined, quarantined);
    counter!(record_coalesced, coalesced);
    counter!(record_upstream_request, upstream_requests);
    counter!(record_upstream_error, upstream_errors);

    /// A reading of all the counters.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        MetricsSnapshot {
            hits: read(&self.hits),
            misses: read(&self.misses),
            revalidated: read(&self.revalidated),
            refreshed: read(&self.refreshed),
            stale: read(&self.stale),
            negative_hits: read(&self.negative_hits),
            offline_hits: read(&self.offline_hits),
            offline_misses: read(&self.offline_misses),
            quarantined: read(&self.quarantined),
            coalesced: read(&self.coalesced),
            upstream_requests: read(&self.upstream_requests),
            upstream_errors: read(&self.upstream_errors),
        }
    }
}

/// Why a resolution failed, in a form that can be shared among
/// the requests that waited for the same download.
///
/// It carries neither the URL nor the upstream response: the client has no
/// business seeing the service's internal network.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProxyFailure {
    /// The upstream does not have that content.
    #[error("the upstream does not have that content")]
    NotFound,
    /// The path falls outside the upstream's scopes.
    #[error("the path is outside the scopes of the upstream")]
    OutOfScope,
    /// There are too many simultaneous egress calls.
    #[error("too many fetches are in flight")]
    Busy,
    /// The upstream did not respond in time.
    #[error("the upstream timed out")]
    Timeout,
    /// The egress policy does not allow going to the upstream.
    #[error("the upstream is not reachable from this service")]
    NotReachable,
    /// The content could not be verified against what the upstream declared.
    #[error("the upstream content could not be verified")]
    Unverified,
    /// The content arrived incomplete or exceeds the cap.
    #[error("the upstream content is incomplete or too large")]
    Incomplete,
    /// The upstream did not deliver (network error, unexpected status, credential).
    #[error("the upstream did not deliver")]
    UpstreamFailed,
    /// The service does not go out to the network.
    #[error("the service is offline")]
    Offline,
    /// The store or the ledger failed: not the upstream's fault.
    #[error("the service could not store the content")]
    Local(LocalFailure),
}

/// A failure of the service itself, with no detail to leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFailure {
    /// The store or the catalog is unavailable.
    Unavailable,
    /// Something broken internally.
    Internal,
    /// The catalog rejected the publication due to a condition.
    Rejected,
}

impl ProxyFailure {
    /// Whether the failure is the upstream's: what counts as an egress error and the
    /// only thing that allows serving expired content within the
    /// `stale_if_error` window.
    ///
    /// The upstream saying `404` does not count: it is an answer, not an error,
    /// and content the upstream withdrew is not kept presented as
    /// current.
    pub fn is_upstream_error(&self) -> bool {
        matches!(
            self,
            Self::Busy
                | Self::Timeout
                | Self::NotReachable
                | Self::Unverified
                | Self::Incomplete
                | Self::UpstreamFailed
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use tokio::sync::Notify;

    use super::*;

    #[tokio::test]
    async fn concurrent_requests_for_one_key_share_a_single_execution() {
        let flights: Arc<SingleFlight<&'static str, u32>> = Arc::new(SingleFlight::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let flights = flights.clone();
            let runs = runs.clone();
            let release = release.clone();
            tasks.push(tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        runs.fetch_add(1, Ordering::SeqCst);
                        release.notified().await;
                        7
                    })
                    .await
            }));
        }
        // Lets all of them arrive and start waiting for the leader.
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        release.notify_one();

        let mut led = 0;
        let mut followed = 0;
        for task in tasks {
            match task.await.expect("the task finishes") {
                Flight::Led(value) => {
                    assert_eq!(value, 7);
                    led += 1;
                }
                Flight::Followed(value) => {
                    assert_eq!(value, 7);
                    followed += 1;
                }
            }
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1, "only one execution");
        assert_eq!((led, followed), (1, 7));
        assert_eq!(flights.in_flight(), 0, "the entry is released");
    }

    #[tokio::test]
    async fn distinct_keys_do_not_wait_for_each_other() {
        let flights: SingleFlight<&'static str, u32> = SingleFlight::new();
        let a = flights.run("a", || async { 1 });
        let b = flights.run("b", || async { 2 });
        let (a, b) = tokio::join!(a, b);
        assert_eq!((a.into_inner(), b.into_inner()), (1, 2));
    }

    #[tokio::test]
    async fn a_finished_flight_does_not_serve_a_later_request() {
        let flights: SingleFlight<&'static str, u32> = SingleFlight::new();
        let first = flights.run("path", || async { 1 }).await;
        let second = flights.run("path", || async { 2 }).await;
        assert_eq!(first, Flight::Led(1));
        assert_eq!(second, Flight::Led(2), "a new request runs again");
    }

    #[tokio::test]
    async fn an_error_is_shared_like_any_other_result() {
        let flights: Arc<SingleFlight<&'static str, Result<u32, ProxyFailure>>> =
            Arc::new(SingleFlight::new());
        let release = Arc::new(Notify::new());
        let leader = {
            let flights = flights.clone();
            let release = release.clone();
            tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        release.notified().await;
                        Err(ProxyFailure::Timeout)
                    })
                    .await
            })
        };
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        let follower = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.run("path", || async { Ok(1) }).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        release.notify_one();
        assert_eq!(
            leader.await.expect("leader"),
            Flight::Led(Err(ProxyFailure::Timeout))
        );
        assert_eq!(
            follower.await.expect("follower"),
            Flight::Followed(Err(ProxyFailure::Timeout))
        );
    }

    #[tokio::test]
    async fn a_cancelled_leader_hands_the_work_to_a_follower() {
        let flights: Arc<SingleFlight<&'static str, u32>> = Arc::new(SingleFlight::new());
        let never = Arc::new(Notify::new());
        let leader = {
            let flights = flights.clone();
            let never = never.clone();
            tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        never.notified().await;
                        1
                    })
                    .await
            })
        };
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        let follower = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.run("path", || async { 2 }).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        // The leader's client leaves: its request is cancelled.
        leader.abort();
        let outcome = tokio::time::timeout(Duration::from_secs(5), follower)
            .await
            .expect("the follower does not hang")
            .expect("follower");
        assert_eq!(outcome, Flight::Led(2), "the follower took over");
        assert_eq!(flights.in_flight(), 0);
    }

    #[test]
    fn an_absence_lasts_its_ttl_and_only_for_its_config_version() {
        let negatives = NegativeCache::new();
        let key = PathKey::of("acme", "central", "org/missing.jar");
        let now = Instant::now();
        negatives.remember(&key, 3, Duration::from_secs(30), now);

        assert!(negatives.contains(&key, 3, now + Duration::from_secs(29)));
        assert!(!negatives.contains(&key, 3, now + Duration::from_secs(30)));
        assert!(
            !negatives.contains(&key, 4, now),
            "a new config version asks again"
        );
        assert!(
            !negatives.contains(&PathKey::of("other", "central", "org/missing.jar"), 3, now),
            "another account does not share it"
        );
    }

    #[test]
    fn a_zero_ttl_remembers_nothing_and_forget_clears_every_version() {
        let negatives = NegativeCache::new();
        let key = PathKey::of("acme", "central", "a");
        let now = Instant::now();
        negatives.remember(&key, 1, Duration::ZERO, now);
        assert!(negatives.is_empty());

        negatives.remember(&key, 1, Duration::from_secs(60), now);
        negatives.remember(&key, 2, Duration::from_secs(60), now);
        negatives.forget(&key);
        assert!(negatives.is_empty());
    }

    #[test]
    fn the_negative_cache_is_bounded_and_purges_expired_entries_first() {
        let negatives = NegativeCache::new();
        let now = Instant::now();
        for index in 0..NEGATIVE_CAPACITY {
            let key = PathKey::of("acme", "central", &format!("p{index}"));
            negatives.remember(&key, 1, Duration::from_secs(10), now);
        }
        assert_eq!(negatives.len(), NEGATIVE_CAPACITY);

        // Full and with no expired entries: nothing more fits.
        let extra = PathKey::of("acme", "central", "extra");
        negatives.remember(&extra, 1, Duration::from_secs(10), now);
        assert!(!negatives.contains(&extra, 1, now));

        // With the earlier ones expired, they are purged and the new one fits.
        let later = now + Duration::from_secs(11);
        negatives.remember(&extra, 1, Duration::from_secs(10), later);
        assert!(negatives.contains(&extra, 1, later));
        assert_eq!(negatives.len(), 1);
    }

    #[test]
    fn the_counters_start_at_zero_and_count_what_happens() {
        let metrics = ProxyMetrics::default();
        assert_eq!(metrics.snapshot(), MetricsSnapshot::default());
        metrics.record_hit();
        metrics.record_hit();
        metrics.record_miss();
        metrics.record_stale();
        metrics.record_upstream_error();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.hits, 2);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.stale, 1);
        assert_eq!(snapshot.upstream_errors, 1);
        assert_eq!(snapshot.quarantined, 0);
    }

    #[test]
    fn only_upstream_failures_allow_a_stale_answer() {
        for failure in [
            ProxyFailure::Busy,
            ProxyFailure::Timeout,
            ProxyFailure::NotReachable,
            ProxyFailure::Unverified,
            ProxyFailure::Incomplete,
            ProxyFailure::UpstreamFailed,
        ] {
            assert!(failure.is_upstream_error(), "{failure:?}");
        }
        for failure in [
            ProxyFailure::NotFound,
            ProxyFailure::OutOfScope,
            ProxyFailure::Offline,
            ProxyFailure::Local(LocalFailure::Internal),
        ] {
            assert!(!failure.is_upstream_error(), "{failure:?}");
        }
    }
}
