//! The admission cache of the trusted channel.
//!
//! The grant ledger lives in the database, and every request on the trusted
//! channel used to be a read of it. This cache stores **only affirmative
//! answers**: an active, fresh grant exempts from asking again;
//! everything else --absence, a ledger failure-- is read live every time, so
//! that no unknown state becomes a permit by cache
//! inertia.
//!
//! The rules that make it safe:
//!
//! - **Keyed by identity, not by name**: account, repository and
//!   digest. A tag that moves to other bytes queries a different key
//!   and does not inherit the old grant.
//! - **Expiration**: each entry lives for a `ttl`; after that the cache goes
//!   back to asking the ledger. It is the staleness ceiling between
//!   instances that do not share writes.
//! - **Invalidation on relevant change**: every write that goes through
//!   here --grant or revocation-- sweeps the entries it touches. A
//!   revocation by this instance is noticed on the next request,
//!   as when there was no cache.
//! - **Generation against in-flight reads**: each write bumps a
//!   counter *before* touching the ledger, and a read whose ledger
//!   answered after that bump does not insert its snapshot: a confirmed
//!   revocation cannot be masked by the late insertion of a
//!   read that started earlier, nor a re-grant buried by the old
//!   snapshot. The bound: a concurrent write costs at most one extra
//!   read of the ledger, never a stale permit. A download already
//!   started is served with the grant that admitted its request --the
//!   revocation cuts new requests, not those already in flight--,
//!   exactly as without a cache.
//! - **Invalidation watermark (HSP-146 / HSP-320)**: this cache is the
//!   only per-process state that can serve a revoked trust, and nodes
//!   that share one Dgraph do not share it. Waiting for the `ttl` would
//!   leave a revoked trust honored for up to 60 s by every other node.
//!   Instead, before serving from an entry the cache reads the
//!   authority's revocation watermark (the count of durable `revoked`
//!   events, which only moves forward) and compares it with the one
//!   stamped on the entry when its authoritative read began; if the
//!   world moved, the entry falls and the ledger is read live. The
//!   watermark read is itself leased for `verify`, so the **bound** on
//!   how long a node may keep honoring a trust revoked by another node
//!   is `verify` plus one probe round trip (`verify = 0` is the strict
//!   profile: every request probes, the cache only shortens the read,
//!   never the exposure). The hard rule: **losing the signal never
//!   renews an ALLOW** --if the watermark cannot be read the whole
//!   cache is dropped, the lease is discarded and the request answers
//!   from the live ledger, which if also down is `Unavailable`--. A
//!   ledger that declares no watermark (`Ok(None)`) keeps the plain
//!   `ttl` semantics.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;

/// The default duration of an entry: how long a revocation made by
/// another instance can take to be noticed.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// The default lease of the watermark check: how often the cache asks the
/// shared authority whether the world of invalidations moved. It is the
/// staleness bound between nodes (the `ttl` stays the bound of the path
/// without signal); `0` is the strict profile.
pub const DEFAULT_VERIFY: Duration = Duration::from_secs(5);

/// The key of a cached decision: the identity under which the ledger
/// grants --account, repository and digest--, not the name the
/// client asked by.
type Key = (String, String, String);

/// An active grant with the time at which it stops being trustworthy.
struct Entry {
    grant: TrustGrant,
    expires_at: Instant,
    /// The watermark observed right before the authoritative read that
    /// produced this entry. `None` is a ledger without signal: the `ttl`
    /// rules.
    stamp: Option<u64>,
}

/// What the shared authority said about the watermark.
#[derive(Clone)]
enum Signal {
    /// The confirmed watermark. An entry stamped with another value was
    /// born before the world moved and falls.
    Current(u64),
    /// The ledger declares no watermark (`Ok(None)`): plain `ttl`.
    Silent,
    /// The probe failed. Nothing cached is renewed.
    Lost,
}

/// The local state of the signal: the last watermark the authority
/// confirmed and when. `checked == None` means "never": the next request
/// probes.
struct SignalState {
    checked: Option<Instant>,
    mark: Signal,
}

/// The ledger wrapped in a positives-only cache.
///
/// It is just another `TrustLedger`: the service state does not distinguish, and the
/// read paths keep calling `admit` just as when there was no
/// cache. Writes invalidate themselves because they pass through here.
pub struct CachedTrustLedger {
    inner: std::sync::Arc<dyn TrustLedger>,
    ttl: Duration,
    /// How often the shared authority's watermark is probed.
    verify: Duration,
    admitted: Mutex<HashMap<Key, Entry>>,
    /// The last confirmed watermark and its confirmation time. A separate
    /// lock from the map: the probe is a cheap read, the map is the
    /// cached permit.
    signal: Mutex<SignalState>,
    /// The counter that every write bumps before touching the ledger. An
    /// in-flight read that saw a generation different from the current one
    /// when the ledger answered it does not insert its snapshot: it may be
    /// stale --the revocation or re-grant is already firm--.
    generation: AtomicU64,
}

impl CachedTrustLedger {
    /// The real ledger, with the ttl each entry must have.
    pub fn new(inner: std::sync::Arc<dyn TrustLedger>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            verify: DEFAULT_VERIFY,
            admitted: Mutex::new(HashMap::new()),
            signal: Mutex::new(SignalState {
                checked: None,
                mark: Signal::Silent,
            }),
            generation: AtomicU64::new(0),
        }
    }

    /// The ttl the production instance is born with.
    pub fn with_default_ttl(inner: std::sync::Arc<dyn TrustLedger>) -> Self {
        Self::new(inner, DEFAULT_TTL)
    }

    /// Sets the lease of the watermark check. Called at startup from the
    /// configuration: `Duration::ZERO` is the strict profile (every request
    /// asks the authority).
    pub fn with_verify(mut self, verify_for: Duration) -> Self {
        self.verify = verify_for;
        self
    }

    fn key(account: &str, repository: &str, digest: &ContentDigest) -> Key {
        (
            account.to_string(),
            repository.to_string(),
            digest.to_string(),
        )
    }

    /// The watermark to stamp the next authoritative read with.
    ///
    /// With the lease standing the confirmed mark is reused; with the lease
    /// expired --or `verify` zero-- the authority is asked. A failed probe
    /// is the lost signal: the whole cache is dropped (no old permit is
    /// left to renew), the lease is discarded, and the request is answered
    /// live from the ledger.
    async fn consult(&self) -> Signal {
        {
            let state = self
                .signal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(checked) = state.checked
                && Instant::now().duration_since(checked) < self.verify
            {
                return state.mark.clone();
            }
        }
        match self.inner.revocation_watermark().await {
            Ok(Some(mark)) => self.settle(Signal::Current(mark), Some(Instant::now())),
            // A ledger without watermark never will have one: renew the
            // lease so as not to probe on every request.
            Ok(None) => self.settle(Signal::Silent, Some(Instant::now())),
            Err(_) => {
                self.fresh().clear();
                self.settle(Signal::Lost, None)
            }
        }
    }

    fn settle(&self, mark: Signal, checked: Option<Instant>) -> Signal {
        let mut state = self
            .signal
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.checked = checked;
        state.mark = mark.clone();
        mark
    }

    /// Forgets the lease after a write of this node: the write moved the
    /// world the authority describes, so the next request looks again.
    fn forget_signal(&self) {
        self.signal
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .checked = None;
    }

    /// Whether the confirmed mark is still the one the snapshot was stamped
    /// with: if a concurrent request re-probed while the ledger answered
    /// and the world moved, the snapshot predates a foreign revocation and
    /// is not inserted.
    fn mark_holds(&self, expected: &Signal) -> bool {
        let state = self
            .signal
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match (&state.mark, expected) {
            (Signal::Current(now), Signal::Current(then)) => now == then,
            (Signal::Silent, Signal::Silent) => true,
            _ => false,
        }
    }

    /// The live entry for the key, if it exists and has not expired.
    fn fresh(&self) -> MutexGuard<'_, HashMap<Key, Entry>> {
        // The mutex may be poisoned if a panicking path left the
        // map half-written: the cache resumes empty, which is the closed
        // side --asking the ledger again--, not a false permit.
        let mut map = self.admitted.lock().unwrap_or_else(|poison| {
            let mut map = poison.into_inner();
            map.clear();
            map
        });
        let now = Instant::now();
        map.retain(|_, entry| entry.expires_at > now);
        map
    }
}

#[async_trait]
impl TrustLedger for CachedTrustLedger {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        // Generation first, then the ledger: a read that started
        // before this point will no longer be able to insert its old snapshot when
        // the ledger answers it.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.grant(grants).await?;
        // The new grant --or the new decision that re-grants something
        // revoked-- invalidates its key: what the cache held was the
        // answer to an earlier question.
        let mut map = self.fresh();
        for grant in grants {
            map.remove(&Self::key(&grant.account, &grant.repository, &grant.digest));
        }
        drop(map);
        self.forget_signal();
        Ok(())
    }

    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_decision(account, repository, decision_id, actor, reason, at)
            .await?;
        self.fresh()
            .retain(|(entry_account, entry_repository, _), entry| {
                !(entry_account == account
                    && entry_repository == repository
                    && entry.grant.decision_id == decision_id)
            });
        self.forget_signal();
        Ok(revoked)
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_decision_for_account(account, decision_id, actor, reason, at)
            .await?;
        self.fresh().retain(|(entry_account, _, _), entry| {
            !(entry_account == account && entry.grant.decision_id == decision_id)
        });
        self.forget_signal();
        Ok(revoked)
    }

    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_digest(account, repository, digest, actor, reason, at)
            .await?;
        self.fresh().remove(&Self::key(account, repository, digest));
        self.forget_signal();
        Ok(revoked)
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        let key = Self::key(account, repository, digest);
        // First the shared authority's signal (HSP-320): free while the
        // lease stands, one cheap read once it expired. A lost signal
        // already dropped the map in `consult`; here it closes the cached
        // path: no cached permit serves while the world cannot be checked.
        let signal = self.consult().await;
        // The cache shortens the trip, not the life of the grant: an entry
        // only serves if the validity of the grant it holds is still
        // standing **at the time asked** and the watermark that stamped it
        // is still the world's. Three clocks --validity, ttl, watermark--;
        // if one lies, the most closed wins.
        if !matches!(signal, Signal::Lost) {
            let map = self.fresh();
            if let Some(entry) = map.get(&key).filter(|entry| {
                entry.grant.is_active_at(at)
                    && match (&entry.stamp, &signal) {
                        (Some(seen), Signal::Current(mark)) => seen == mark,
                        (None, Signal::Silent) => true,
                        // A mismatch of signal kinds: the closed side is
                        // to read again.
                        _ => false,
                    }
            }) {
                return Ok(Some(entry.grant.clone()));
            }
        }
        let gen_before = self.generation.load(Ordering::SeqCst);
        let stamp = match &signal {
            Signal::Current(mark) => Some(*mark),
            _ => None,
        };
        let grant = self
            .inner
            .active_grant(account, repository, digest, at)
            .await?;
        // Only the **current** affirmation is cached: absence, ledger
        // failure and expired entries are always asked again.
        if let Some(active) = grant.as_ref().filter(|grant| grant.is_active_at(at)) {
            // While holding the map: if a write of this node bumped the
            // generation, or the authority moved the watermark while the
            // ledger answered, the snapshot may predate a revocation and is
            // not inserted. It costs at most one extra read of the ledger,
            // never a stale permit.
            let mut map = self.fresh();
            if gen_before == self.generation.load(Ordering::SeqCst) && self.mark_holds(&signal) {
                map.insert(
                    key,
                    Entry {
                        grant: active.clone(),
                        expires_at: Instant::now() + self.ttl,
                        stamp,
                    },
                );
            }
        }
        Ok(grant)
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        self.inner.grants_of(account, repository).await
    }

    /// The book's own mark, unchanged: the wrapper adds no invalidations of
    /// its own that the authority would not know.
    async fn revocation_watermark(&self) -> Result<Option<u64>, CatalogError> {
        self.inner.revocation_watermark().await
    }

    async fn active_digests(
        &self,
        account: &str,
        repository: &str,
        digests: &[ContentDigest],
        at: DateTime<Utc>,
    ) -> Result<Vec<ContentDigest>, CatalogError> {
        // A listing reads the ledger itself: the page is answered by one
        // bounded read of the inner ledger, and a listing does not fill a
        // per-digest cache with the digests it merely names.
        self.inner
            .active_digests(account, repository, digests, at)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// The tests' ledger: it remembers its reads and denies whatever is
    /// put in front of it, with no database behind it.
    struct FakeLedger {
        grants: tokio::sync::Mutex<Vec<TrustGrant>>,
        reads: AtomicUsize,
        /// The watermark the ledger answers: `SILENT` (no signal), `LOST`
        /// (the probe fails) or a confirmed mark.
        mark: std::sync::atomic::AtomicI64,
    }

    const SILENT: i64 = -1;
    const LOST: i64 = -2;

    impl FakeLedger {
        fn new(grants: Vec<TrustGrant>) -> Self {
            Self {
                grants: tokio::sync::Mutex::new(grants),
                reads: AtomicUsize::new(0),
                mark: std::sync::atomic::AtomicI64::new(SILENT),
            }
        }

        fn set_mark(&self, mark: i64) {
            self.mark.store(mark, Ordering::SeqCst);
        }

        fn the_reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    fn a_grant(digest: &ContentDigest) -> TrustGrant {
        TrustGrant {
            account: "acme".to_string(),
            repository: "trusted-repo".to_string(),
            digest: digest.clone(),
            decision_id: "dec-1".to_string(),
            actor: "promoter".to_string(),
            granted_at: Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_string()),
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: Some("sha256:capsule".to_string()),
            risk_version: Some(0),
            valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
        }
    }

    #[async_trait]
    impl TrustLedger for FakeLedger {
        async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
            self.grants.lock().await.extend_from_slice(grants);
            Ok(())
        }

        async fn revoke_decision(
            &self,
            _account: &str,
            _repository: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if grant.decision_id == decision_id && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_decision_for_account(
            &self,
            account: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if grant.account == account
                    && grant.decision_id == decision_id
                    && grant.revoked_at.is_none()
                {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_digest(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if &grant.digest == digest && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn active_grant(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            at: DateTime<Utc>,
        ) -> Result<Option<TrustGrant>, CatalogError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .grants
                .lock()
                .await
                .iter()
                .find(|grant| &grant.digest == digest && grant.is_active_at(at))
                .cloned())
        }

        async fn grants_of(
            &self,
            _account: &str,
            _repository: &str,
        ) -> Result<Vec<TrustGrant>, CatalogError> {
            Ok(self.grants.lock().await.clone())
        }

        async fn revocation_watermark(&self) -> Result<Option<u64>, CatalogError> {
            match self.mark.load(Ordering::SeqCst) {
                SILENT => Ok(None),
                LOST => Err(CatalogError::unavailable("the signal is lost")),
                mark => Ok(Some(mark as u64)),
            }
        }
    }

    fn the_digest_of(label: &str) -> ContentDigest {
        ContentDigest::of_bytes(label.as_bytes())
    }

    /// The raison d'etre of the cache: the second request for the same
    /// digest does not go back to the ledger.
    #[tokio::test]
    async fn an_admitted_digest_is_asked_to_the_book_once() {
        let digest = the_digest_of("admitted bytes");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for _ in 0..3 {
            let grant = cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la cache responde");
            assert!(grant.expect("hay concesion").is_active_at(Utc::now()));
        }
        assert_eq!(fake.the_reads(), 1, "dos de tres lecturas fueron por cache");
    }

    /// Security AC: a revocation by this instance is noticed on the
    /// next request, without waiting for the ttl.
    #[tokio::test]
    async fn a_revocation_takes_effect_on_the_next_request() {
        let digest = the_digest_of("revoked bytes");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la primera concede")
                .is_some()
        );

        cached
            .revoke_digest(
                "acme",
                "trusted-repo",
                &digest,
                "gatekeeper",
                "CVE-2026-0001",
                Utc::now(),
            )
            .await
            .expect("la revocacion pasa por la cache");

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la segunda pregunta")
                .is_none(),
            "revocado, la cache no lo recuerda"
        );
    }

    /// Revocation by decision sweeps all its keys: the decision
    /// promoted several digests at once.
    #[tokio::test]
    async fn revoking_a_decision_clears_every_digest_it_granted() {
        let first = the_digest_of("first");
        let second = the_digest_of("second");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&first), a_grant(&second)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for digest in [&first, &second] {
            cached
                .active_grant("acme", "trusted-repo", digest, Utc::now())
                .await
                .expect("ambos concedidos");
        }
        cached
            .revoke_decision(
                "acme",
                "trusted-repo",
                "dec-1",
                "gatekeeper",
                "policy",
                Utc::now(),
            )
            .await
            .expect("revocar la decision");

        for digest in [&first, &second] {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", digest, Utc::now())
                    .await
                    .expect("la cache vuelve al libro")
                    .is_none()
            );
        }
    }

    /// AC1: the key is the digest. Moving the tag to other bytes
    /// asks for a key the cache does not have: yesterday's decision
    /// about `sha256:X` says nothing about `sha256:Y`.
    #[tokio::test]
    async fn a_moved_tag_does_not_inherit_the_cached_decision() {
        let old_bytes = the_digest_of("the bytes the tag had");
        let new_bytes = the_digest_of("the bytes the tag moved to");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&old_bytes)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &old_bytes, Utc::now())
                .await
                .expect("lo viejo esta concedido")
                .is_some()
        );

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &new_bytes, Utc::now())
                .await
                .expect("lo nuevo se pregunta al libro")
                .is_none(),
            "la cache no presta su si a otros bytes"
        );
    }

    /// What is denied is asked every time: promoting something new from another
    /// instance is served on the next request.
    #[tokio::test]
    async fn negatives_are_never_cached() {
        let digest = the_digest_of("not yet admitted");
        let fake = std::sync::Arc::new(FakeLedger::new(Vec::new()));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for _ in 0..2 {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
                    .expect("el libro dice que no")
                    .is_none()
            );
        }
        assert_eq!(fake.the_reads(), 2, "cada no vuelve a preguntar");

        // And when another instance promotes, it shows up here with no ttl in between.
        fake.grant(&[a_grant(&digest)])
            .await
            .expect("promocion ajena");
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la promocion se sirve")
                .is_some()
        );
    }

    /// The ttl is the staleness ceiling: once the entry expires, the
    /// cache asks again, and if the ledger died in the meantime, the
    /// question is made and fails --a failure is never a permit.
    #[tokio::test]
    async fn an_expired_entry_asks_the_book_again() {
        let digest = the_digest_of("expired admission");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::ZERO);

        for _ in 0..2 {
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("con ttl cero nada es fresco");
        }
        assert_eq!(fake.the_reads(), 2, "el ttl expirado iguala al libro vivo");
    }

    /// The ledger that lets itself be blocked inside the read, to pin the
    /// race between a read started before the revocation and its
    /// later insertion.
    struct BlockyLedger {
        grants: tokio::sync::Mutex<Vec<TrustGrant>>,
        entered: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    }

    impl BlockyLedger {
        fn new(grants: Vec<TrustGrant>) -> Self {
            Self {
                grants: tokio::sync::Mutex::new(grants),
                entered: tokio::sync::Mutex::new(None),
                release: tokio::sync::Mutex::new(None),
            }
        }

        /// Arms a barrier: the next read signals and waits.
        async fn arm(
            &self,
        ) -> (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ) {
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            *self.entered.lock().await = Some(entered_tx);
            *self.release.lock().await = Some(release_rx);
            (entered_rx, release_tx)
        }
    }

    #[async_trait]
    impl TrustLedger for BlockyLedger {
        async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
            *self.grants.lock().await = grants.to_vec();
            Ok(())
        }

        async fn revoke_decision(
            &self,
            account: &str,
            _repository: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut revoked = 0;
            for grant in self.grants.lock().await.iter_mut() {
                if grant.account == account
                    && grant.decision_id == decision_id
                    && grant.revoked_at.is_none()
                {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_decision_for_account(
            &self,
            account: &str,
            decision_id: &str,
            actor: &str,
            reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            self.revoke_decision(account, "trusted-repo", decision_id, actor, reason, at)
                .await
        }

        async fn revoke_digest(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut revoked = 0;
            for grant in self.grants.lock().await.iter_mut() {
                if &grant.digest == digest && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn active_grant(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            at: DateTime<Utc>,
        ) -> Result<Option<TrustGrant>, CatalogError> {
            // The snapshot is taken **before** the barrier: a ledger read
            // started before the revocation sees the state there was
            // when it began, and the problem is where the caching wrapper
            // inserts that late snapshot.
            let snapshot = {
                let grants = self.grants.lock().await;
                grants
                    .iter()
                    .find(|grant| &grant.digest == digest && grant.is_active_at(at))
                    .cloned()
            };
            if let Some(sender) = self.entered.lock().await.take() {
                let _ = sender.send(());
                if let Some(waiter) = self.release.lock().await.take() {
                    let _ = waiter.await;
                }
            }
            Ok(snapshot)
        }

        async fn grants_of(
            &self,
            _account: &str,
            _repo: &str,
        ) -> Result<Vec<TrustGrant>, CatalogError> {
            Ok(self.grants.lock().await.clone())
        }
    }

    /// AC1: the race reproduced with deterministic barriers. A
    /// read started **before** the revocation cannot reopen the
    /// grant in the cache after its confirmation.
    #[tokio::test]
    async fn a_read_started_before_a_confirmed_revoke_never_admits_again() {
        let digest = the_digest_of("revoked mid-read");
        let book = std::sync::Arc::new(BlockyLedger::new(vec![a_grant(&digest)]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let reader = {
            let cached = cached.clone();
            let digest = digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the read reached the book");

        cached
            .revoke_digest(
                "acme",
                "trusted-repo",
                &digest,
                "gatekeeper",
                "CVE-2026-0001",
                Utc::now(),
            )
            .await
            .expect("the revoke is confirmed while the read is in flight");
        release.send(()).expect("the blocked read is released");

        let in_flight = reader.await.expect("the reader task finished");
        assert!(
            in_flight.expect("the in-flight read answered").is_some(),
            "la peticion en vuelo se atiende con su snapshot"
        );

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("the next query goes to the book")
                .is_none(),
            "AC1: ninguna consulta iniciada tras la confirmacion reabre la concesion vieja"
        );
    }

    /// The same race with a re-grant: a new grant written
    /// during the old read cannot be masked by the late insertion
    /// of the earlier decision.
    #[tokio::test]
    async fn a_regrant_written_mid_read_is_not_buried_by_the_stale_insert() {
        let digest = the_digest_of("regranted mid-read");
        let book = std::sync::Arc::new(BlockyLedger::new(vec![a_grant(&digest)]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let reader = {
            let cached = cached.clone();
            let digest = digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the read reached the book");

        let mut fresh = a_grant(&digest);
        fresh.decision_id = "dec-2".to_string();
        cached
            .grant(&[fresh])
            .await
            .expect("the regrant lands mid-read");
        release.send(()).expect("the blocked read is released");
        reader
            .await
            .expect("the reader task finished")
            .expect("the in-flight read answered");

        let found = cached
            .active_grant("acme", "trusted-repo", &digest, Utc::now())
            .await
            .expect("the ledger answers");
        assert_eq!(
            found.expect("the fresh grant admits").decision_id,
            "dec-2",
            "la decision nueva no queda enterrada por la insercion vieja"
        );
    }

    /// AC3: the revocation of an account does not tear down the entries nor the
    /// progress of in-flight reads of another account.
    #[tokio::test]
    async fn revoking_one_account_leaves_another_in_flight_untouched() {
        let mine = a_grant(&the_digest_of("mine"));
        let mut theirs = a_grant(&the_digest_of("theirs"));
        theirs.account = "globex".to_string();
        let book = std::sync::Arc::new(BlockyLedger::new(vec![mine, theirs.clone()]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let theirs_digest = theirs.digest.clone();
        let reader = {
            let cached = cached.clone();
            let theirs_digest = theirs_digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("globex", "trusted-repo", &theirs_digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the globex read is in flight");

        cached
            .revoke_decision(
                "acme",
                "trusted-repo",
                "dec-1",
                "gatekeeper",
                "policy",
                Utc::now(),
            )
            .await
            .expect("the acme revoke is confirmed");
        release.send(()).expect("the globex read is released");
        let _ = reader.await.expect("the reader task finished");

        assert!(
            cached
                .active_grant("globex", "trusted-repo", &theirs_digest, Utc::now())
                .await
                .expect("globex still admitted")
                .is_some(),
            "AC3: revocar en acme no invalida a globex"
        );
    }

    /// AC2, and the separation the contract requires: the cache shortens the
    /// trip, it does not lengthen the life. An entry with a one-hour ttl does not
    /// serve a grant whose validity expired.
    #[tokio::test]
    async fn the_cache_does_not_extend_the_validity_of_a_grant() {
        let digest = the_digest_of("short lived admission");
        let mut grant = a_grant(&digest);
        grant.valid_until = Some(Utc::now() + chrono::Duration::minutes(5));
        let fake = std::sync::Arc::new(FakeLedger::new(vec![grant]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(3600));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("viva, se sirve")
                .is_some()
        );

        let later = Utc::now() + chrono::Duration::hours(1);
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, later)
                .await
                .expect("caducada, el libro lo dice")
                .is_none(),
            "la cache no presta vida a un grant caducado"
        );
        assert_eq!(fake.the_reads(), 2, "y la caducidad se comprueba de nuevo");
    }

    /// HSP-320: a revocation confirmed behind this cache's back (another
    /// node) moves the watermark; the stamped entry falls on the next
    /// request instead of waiting for the ttl.
    #[tokio::test]
    async fn a_foreign_revocation_drops_the_entry_when_the_watermark_moves() {
        let digest = the_digest_of("foreign revocation");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        fake.set_mark(7);
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60))
            .with_verify(Duration::ZERO);

        for _ in 0..2 {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
                    .expect("served")
                    .is_some()
            );
        }
        assert_eq!(fake.the_reads(), 1, "the second request was a cache hit");

        // Another node revokes: the ledger changes and the mark moves.
        fake.revoke_digest("acme", "trusted-repo", &digest, "other", "cve", Utc::now())
            .await
            .expect("foreign revocation");
        fake.set_mark(8);

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("answers")
                .is_none(),
            "the revoked trust is not honored after the watermark moved"
        );
    }

    /// HSP-320: within the lease the cache does not probe; past it, it does.
    /// The lease is the bound of the exposure.
    #[tokio::test]
    async fn the_lease_bounds_the_exposure_to_a_foreign_revocation() {
        let digest = the_digest_of("lease");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        fake.set_mark(1);
        let lease = Duration::from_millis(200);
        let cached =
            CachedTrustLedger::new(fake.clone(), Duration::from_secs(60)).with_verify(lease);

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("served")
                .is_some()
        );
        fake.revoke_digest("acme", "trusted-repo", &digest, "other", "cve", Utc::now())
            .await
            .expect("foreign revocation");
        fake.set_mark(2);

        tokio::time::sleep(lease + Duration::from_millis(50)).await;
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("answers")
                .is_none(),
            "past the lease the foreign revocation is honored"
        );
    }

    /// HSP-320 hard rule: a lost signal never renews a cached ALLOW. The
    /// cache is dropped and the request goes to the ledger; if the ledger
    /// also fails, the answer is an error, never a permit.
    #[tokio::test]
    async fn a_lost_signal_never_renews_a_cached_allow() {
        let digest = the_digest_of("lost signal");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        fake.set_mark(1);
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60))
            .with_verify(Duration::ZERO);
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("served")
                .is_some()
        );
        assert_eq!(fake.the_reads(), 1);

        fake.set_mark(LOST);
        // The signal is lost: the entry is NOT served from the cache; the
        // request reads the live ledger (a second read), which still holds.
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("the live ledger answers")
                .is_some()
        );
        assert_eq!(
            fake.the_reads(),
            2,
            "no cached permit served on a lost signal"
        );

        // And the snapshot of the lost-signal read was not inserted: once
        // the signal returns, with a moved world, nothing stale survives.
        fake.revoke_digest("acme", "trusted-repo", &digest, "other", "cve", Utc::now())
            .await
            .expect("foreign revocation");
        fake.set_mark(2);
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("answers")
                .is_none()
        );
    }

    /// A ledger without signal keeps the plain ttl semantics.
    #[tokio::test]
    async fn a_silent_ledger_keeps_the_plain_ttl_semantics() {
        let digest = the_digest_of("silent");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60))
            .with_verify(Duration::ZERO);
        for _ in 0..3 {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
                    .expect("served")
                    .is_some()
            );
        }
        assert_eq!(fake.the_reads(), 1, "the ttl rules without a signal");
    }
}
