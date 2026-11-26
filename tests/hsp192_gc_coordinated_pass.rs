//! The fenced coordinated pass, over a test ledger and object store
//! (HSP-192).
//!
//! The ledger simulates the authority: the claim lives in the ledger, not in the
//! worker; the fence is the current token, and every pass operation
//! presents it. The object store simulates the store's capabilities:
//! versioning with a marker, conditional delete and quarantine, so as to be able to
//! provoke the concurrent rewrite that AC2 demands never be overwritten.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_artifacts_api::gc::{CoordinatedPass, GcError, GcService, PassAction, PassConfig};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcLease,
    GcRun, Quarantined, RootKind, RootPage, RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{
    BlobError, BlobReclaim, BlobStore, ConditionalBlob, ConditionalOutcome, ReadPolicy, Reclaimed,
    StoredBlob, VersionMarker,
};

const GRACE: Duration = Duration::from_secs(60);
const SCOPE: &str = "hrn:hodei:account:acme";

fn age() -> Duration {
    // The test blobs are always old: grace is not what
    // is being tested here.
    GRACE * 10
}

// ---------------------------------------------------------------------------
// The test object store: versioned CAS, with a marker per version.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreMode {
    /// Versions with a marker and safe conditional delete.
    Conditional,
    /// Versions with a marker, but conditional delete is NOT safe:
    /// the only honest way out is versioned quarantine.
    QuarantineOnly,
    /// Neither markers nor condition: the store cannot offer a guard.
    Legacy,
}

struct CasInner {
    /// digest -> stack of versions (marker, bytes), the last one current.
    versions: HashMap<String, Vec<(String, Vec<u8>)>>,
    /// The quarantine: new key -> bytes.
    quarantined: HashMap<String, Vec<u8>>,
    counter: u64,
}

struct MockCas {
    inner: Mutex<CasInner>,
    mode: StoreMode,
}

impl MockCas {
    fn new(mode: StoreMode) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CasInner {
                versions: HashMap::new(),
                quarantined: HashMap::new(),
                counter: 0,
            }),
            mode,
        })
    }

    fn put_version(&self, digest: &ContentDigest, bytes: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.counter += 1;
        let marker = format!("v{}", inner.counter);
        inner
            .versions
            .entry(digest.value.clone())
            .or_default()
            .push((marker, bytes.to_vec()));
    }

    fn head(&self, digest: &ContentDigest) -> Option<(String, Vec<u8>)> {
        let inner = self.inner.lock().unwrap();
        inner
            .versions
            .get(&digest.value)
            .and_then(|stack| stack.last())
            .cloned()
    }

    fn has_key(&self, digest: &ContentDigest) -> bool {
        self.head(digest).is_some()
    }

    fn quarantine_of(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().quarantined.get(key).cloned()
    }

    /// Rewrites the key with other content: it is the race that AC2
    /// forbids overwriting.
    fn republish(&self, digest: &ContentDigest, bytes: &[u8]) {
        self.put_version(digest, bytes);
    }
}

#[async_trait]
impl BlobStore for MockCas {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        self.put_version(&digest, bytes);
        Ok(digest)
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.head(digest)
            .map(|(_, bytes)| bytes)
            .ok_or(BlobError::NotFound)
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        Ok(self.head(digest).is_some())
    }
    async fn begin(&self) -> Result<Box<dyn hodei_hub::BlobWriteSession>, BlobError> {
        Err(BlobError::Unavailable("not needed".to_string()))
    }
    async fn get_with(
        &self,
        digest: &ContentDigest,
        _policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        self.get(digest).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        Ok(self.head(digest).map_or(0, |(_, bytes)| bytes.len() as u64))
    }
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        let bytes = self.get(digest).await?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
        bytes
            .get(start.min(bytes.len())..end.min(bytes.len()))
            .map(<[u8]>::to_vec)
            .ok_or(BlobError::NotFound)
    }
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        if self.head(digest).is_some() {
            Ok(())
        } else {
            Err(BlobError::NotFound)
        }
    }
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        Some(self)
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        match self.mode {
            StoreMode::Legacy => None,
            _ => Some(self),
        }
    }
}

#[async_trait]
impl BlobReclaim for MockCas {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .versions
            .iter()
            .filter_map(|(hex, stack)| {
                let (marker, bytes) = stack.last()?;
                let _ = marker;
                Some(StoredBlob {
                    digest: ContentDigest::sha256(hex),
                    length: bytes.len() as u64,
                    age: age(),
                })
            })
            .collect())
    }
    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        _grace: Duration,
    ) -> Result<Reclaimed, BlobError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(stack) = inner.versions.get_mut(&digest.value) else {
            return Ok(Reclaimed::Gone);
        };
        match stack.pop() {
            Some((_, bytes)) => {
                if stack.is_empty() {
                    inner.versions.remove(&digest.value);
                }
                Ok(Reclaimed::Deleted {
                    length: bytes.len() as u64,
                })
            }
            None => {
                inner.versions.remove(&digest.value);
                Ok(Reclaimed::Gone)
            }
        }
    }
}

#[async_trait]
impl ConditionalBlob for MockCas {
    fn supports_conditional_delete(&self) -> bool {
        self.mode == StoreMode::Conditional
    }
    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<VersionMarker>, BlobError> {
        Ok(self.head(digest).map(|(marker, bytes)| VersionMarker {
            key: digest.to_string(),
            marker,
            length: bytes.len() as u64,
        }))
    }
    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(stack) = inner.versions.get_mut(&digest.value) else {
            return Ok(ConditionalOutcome::Gone);
        };
        match stack.last() {
            Some((head, _)) if *head != marker.marker => Ok(ConditionalOutcome::NotApplied),
            Some((_, bytes)) => {
                let length = bytes.len() as u64;
                stack.pop();
                if stack.is_empty() {
                    inner.versions.remove(&digest.value);
                }
                Ok(ConditionalOutcome::Applied { length })
            }
            None => Ok(ConditionalOutcome::Gone),
        }
    }
    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(stack) = inner.versions.get(&digest.value) else {
            return Ok(ConditionalOutcome::Gone);
        };
        let Some((head, bytes)) = stack.last().cloned() else {
            return Ok(ConditionalOutcome::Gone);
        };
        if head != marker.marker {
            return Ok(ConditionalOutcome::NotApplied);
        }
        let length = bytes.len() as u64;
        inner.quarantined.insert(quarantine_key.to_string(), bytes);
        let stack = inner
            .versions
            .get_mut(&digest.value)
            .expect("the key is present by check above");
        stack.pop();
        if stack.is_empty() {
            inner.versions.remove(&digest.value);
        }
        Ok(ConditionalOutcome::Applied { length })
    }
}

// ---------------------------------------------------------------------------
// The test fenced ledger: the claim lives here, as in the authority.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct LeaseRow {
    generation: u64,
    token: String,
    owner: String,
    lease_until: DateTime<Utc>,
}

#[derive(Default)]
struct Inner {
    roots: Vec<ContentRoot>,
    holds: Vec<ContentHold>,
    runs: Vec<GcRun>,
    tombstones: Vec<GcDeletion>,
    quarantines: Vec<Quarantined>,
    /// scope -> current claim.
    leases: HashMap<String, LeaseRow>,
    /// scope -> last generation issued (even if expired).
    generations: HashMap<String, u64>,
    tokens: u64,
    /// The history of issued claims, to resurrect old tokens.
    issued: Vec<(String, GcLease)>,
    page_calls: Vec<usize>,
    /// Sabotage: the renew fails from the nth pass onward.
    renew_failures_after: Option<usize>,
    renew_calls: usize,
    /// Sabotage: an action triggered when an intent is recorded.
    on_record: Option<Box<dyn Fn() + Send + Sync>>,
}

#[derive(Default)]
struct MockLedger {
    inner: Mutex<Inner>,
}

impl MockLedger {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn add_root(&self, digest: &ContentDigest, subject: &str) {
        self.inner.lock().unwrap().roots.push(ContentRoot {
            kind: RootKind::Reference,
            account: "acme".to_string(),
            repository: "repo".to_string(),
            subject: subject.to_string(),
            digest: digest.clone(),
        });
    }

    fn force_expire(&self) {
        let mut inner = self.inner.lock().unwrap();
        for lease in inner.leases.values_mut() {
            lease.lease_until = Utc::now() - Duration::from_secs(1);
        }
    }

    fn issued_lease(&self, generation: u64) -> GcLease {
        let inner = self.inner.lock().unwrap();
        inner
            .issued
            .iter()
            .find(|(_, lease)| lease.generation == generation)
            .map(|(_, lease)| lease.clone())
            .expect("a lease of that generation was issued")
    }

    fn fail_renewals_after(&self, calls: usize) {
        self.inner.lock().unwrap().renew_failures_after = Some(calls);
    }

    fn on_record_intention(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.inner.lock().unwrap().on_record = Some(Box::new(hook));
    }

    fn tombstone(&self, digest: &ContentDigest) -> Option<GcDeletion> {
        let inner = self.inner.lock().unwrap();
        inner
            .tombstones
            .iter()
            .find(|trace| &trace.digest == digest)
            .cloned()
    }

    fn quarantine_of(&self, digest: &ContentDigest) -> Option<Quarantined> {
        let inner = self.inner.lock().unwrap();
        inner
            .quarantines
            .iter()
            .find(|item| &item.digest == digest)
            .cloned()
    }

    fn max_page_calls(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .page_calls
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The authority's fence: whoever presents a token that is not the
    /// scope's current one receives PreconditionFailed.
    fn fence(
        inner: &Inner,
        scope: &str,
        lease: &GcLease,
        now: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        match inner.leases.get(scope) {
            Some(row)
                if row.lease_until > now
                    && row.token == lease.token
                    && row.generation == lease.generation =>
            {
                Ok(())
            }
            Some(row) => Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                format!(
                    "token presented for generation {} but the scope is held by generation {}",
                    lease.generation, row.generation
                ),
            )),
            None => Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the scope has no live lease",
            )),
        }
    }
}

#[async_trait]
impl ContentLedger for MockLedger {
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        Ok(self.inner.lock().unwrap().roots.clone())
    }
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        Ok(self.inner.lock().unwrap().holds.clone())
    }
    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        inner.holds.retain(|existing| existing.id != hold.id);
        inner.holds.push(hold.clone());
        Ok(())
    }
    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.holds.len();
        inner.holds.retain(|hold| hold.id != id);
        Ok(before != inner.holds.len())
    }
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.state == RunState::Marked)
            .cloned())
    }
    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.state == RunState::Swept)
            .cloned())
    }
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        inner.runs.retain(|existing| existing.id != run.id);
        inner.runs.push(run.clone());
        Ok(())
    }
    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        for run in inner.runs.iter_mut().filter(|run| run.id == run_id) {
            run.state = RunState::Swept;
            run.deleted = deleted;
            run.bytes_freed = bytes_freed;
            run.swept_at = Some(at);
        }
        Ok(())
    }
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .tombstones
            .retain(|existing| existing.digest != deletion.digest);
        inner.tombstones.push(deletion.clone());
        Ok(())
    }
    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .tombstones
            .iter()
            .find(|trace| &trace.digest == digest)
            .cloned())
    }

    // -- coordinated ---------------------------------------------------------

    fn supports_fenced_pass(&self) -> bool {
        true
    }

    async fn claim_gc_pass(
        &self,
        scope: &str,
        owner: &str,
        lease: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        // It is first decided against the current state, without holding the
        // borrow across the mutation.
        enum Decision {
            Renew(GcLease),
            Busy,
            TakeOver,
        }
        let decision = match inner.leases.get(scope) {
            Some(row) if row.lease_until > now => {
                if row.owner == owner {
                    Decision::Renew(GcLease {
                        generation: row.generation,
                        token: row.token.clone(),
                        lease_until: now + lease,
                    })
                } else {
                    Decision::Busy
                }
            }
            _ => Decision::TakeOver,
        };
        if let Decision::Busy = decision {
            let row = inner.leases.get(scope).expect("busy implies a row");
            return Err(CatalogError::conflict(
                "gc lease",
                scope,
                format!("held by {} until {}", row.owner, row.lease_until),
            ));
        }
        let issued = match decision {
            Decision::Renew(back) => {
                if let Some(row) = inner.leases.get_mut(scope) {
                    row.lease_until = back.lease_until;
                }
                back
            }
            _ => {
                // Expired or nonexistent: NEW generation and NEW token.
                // This is what fences the zombie of the previous
                // generation.
                let next = inner.generations.get(scope).copied().unwrap_or(0) + 1;
                inner.tokens += 1;
                let back = GcLease {
                    generation: next,
                    token: format!("token-{}", inner.tokens),
                    lease_until: now + lease,
                };
                inner.generations.insert(scope.to_string(), next);
                inner.leases.insert(
                    scope.to_string(),
                    LeaseRow {
                        generation: next,
                        token: back.token.clone(),
                        owner: owner.to_string(),
                        lease_until: back.lease_until,
                    },
                );
                back
            }
        };
        inner.issued.push((scope.to_string(), issued.clone()));
        Ok(issued)
    }

    async fn current_gc_lease(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .leases
            .get(scope)
            .and_then(|row| {
                (row.lease_until > now).then(|| GcLease {
                    generation: row.generation,
                    token: row.token.clone(),
                    lease_until: row.lease_until,
                })
            }))
    }

    async fn mark_run_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
        candidate_digests: &[ContentDigest],
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, Utc::now())?;
        // The references guard, inside the same "transaction":
        // a candidate with a live name aborts the whole mark.
        if let Some(hit) = candidate_digests
            .iter()
            .find(|digest| inner.roots.iter().any(|root| &root.digest == *digest))
        {
            return Err(CatalogError::precondition_failed(
                "gc mark",
                hit.to_string(),
                "a candidate gained a live reference while marking",
            ));
        }
        let stamped = GcRun {
            scope: Some(scope.to_string()),
            generation: Some(lease.generation),
            ..run.clone()
        };
        let candidates = stamped.candidates.clone();
        inner.runs.retain(|existing| existing.id != stamped.id);
        inner.runs.push(stamped);
        Ok(candidates)
    }

    async fn mark_of_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        let inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, Utc::now())?;
        Ok(inner
            .runs
            .iter()
            .find(|run| run.id == run_id && run.state == RunState::Marked)
            .cloned())
    }

    async fn record_deletion_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, Utc::now())?;
        if deletion.state == DeletionState::Intended
            && let Some(hook) = &inner.on_record
        {
            hook();
        }
        inner
            .tombstones
            .retain(|existing| existing.digest != deletion.digest);
        inner.tombstones.push(deletion.clone());
        Ok(())
    }

    async fn record_quarantine(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, Utc::now())?;
        inner.quarantines.push(quarantine.clone());
        Ok(())
    }

    async fn root_page(
        &self,
        _scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        inner.page_calls.push(limit);
        let mut roots = inner.roots.clone();
        roots.sort_by_key(|root| root.digest.to_string());
        let start = match after {
            Some(cursor) => roots
                .iter()
                .position(|root| root.digest.to_string() == cursor)
                .map_or(0, |index| index + 1),
            None => 0,
        };
        let page: Vec<ContentRoot> = roots.into_iter().skip(start).take(limit).collect();
        let has_more = page.len() == limit && {
            let remaining = inner.roots.len() - start - page.len();
            remaining > 0
        };
        Ok(RootPage {
            roots: page,
            has_more,
        })
    }

    async fn quarantines_of_run(&self, run_id: &str) -> Result<Vec<Quarantined>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .quarantines
            .iter()
            .filter(|item| item.run_id == run_id)
            .cloned()
            .collect())
    }

    async fn pending_scoped_run(
        &self,
        scope: &str,
        _now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.scope.as_deref() == Some(scope) && run.state == RunState::Marked)
            .cloned())
    }

    async fn renew_gc_lease(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, now)?;
        inner.renew_calls += 1;
        if inner.renew_failures_after == Some(inner.renew_calls) {
            // Sabotage: the scope moved to another generation mid-way
            // through the sweep (the zombie does not continue).
            let next = inner.generations.get(scope).copied().unwrap_or(0) + 1;
            inner.generations.insert(scope.to_string(), next);
            inner.tokens += 1;
            let token = format!("token-{}", inner.tokens);
            inner.leases.insert(
                scope.to_string(),
                LeaseRow {
                    generation: next,
                    token,
                    owner: "take-over".to_string(),
                    lease_until: now + lease_for,
                },
            );
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the lease moved to a new generation",
            ));
        }
        let until = now + lease_for;
        if let Some(row) = inner.leases.get_mut(scope) {
            row.lease_until = until;
            row.owner = owner.to_string();
        }
        Ok(GcLease {
            generation: lease.generation,
            token: lease.token.clone(),
            lease_until: until,
        })
    }

    async fn live_reference_count(&self, digest: &ContentDigest) -> Result<u64, CatalogError> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .roots
            .iter()
            .filter(|root| &root.digest == digest)
            .count() as u64)
    }

    async fn complete_scoped_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        quarantined: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let mut inner = self.inner.lock().unwrap();
        Self::fence(&inner, scope, lease, at)?;
        let _ = quarantined;
        for run in inner.runs.iter_mut().filter(|run| run.id == run_id) {
            run.state = RunState::Swept;
            run.deleted = deleted;
            run.bytes_freed = bytes_freed;
            run.swept_at = Some(at);
        }
        Ok(())
    }
}

struct World {
    service: Arc<GcService>,
    ledger: Arc<MockLedger>,
    store: Arc<MockCas>,
}

fn world(mode: StoreMode) -> World {
    let ledger = MockLedger::new();
    let store = MockCas::new(mode);
    let service = Arc::new(
        GcService::new(
            Arc::clone(&ledger) as Arc<dyn ContentLedger>,
            Arc::clone(&store) as Arc<dyn BlobStore>,
            GRACE,
        )
        .expect("the mock store reclaims"),
    );
    World {
        service,
        ledger,
        store,
    }
}

fn pass(w: &World, owner: &str) -> CoordinatedPass {
    CoordinatedPass::new(
        Arc::clone(&w.service),
        PassConfig::new(SCOPE, owner, Duration::from_secs(30)),
    )
}

// ---------------------------------------------------------------------------
// AC1: the fenced claim, the zombie and the concurrent publisher.
// ---------------------------------------------------------------------------

/// A live foreign claim occupies the scope: the second worker does not sweep.
#[tokio::test]
async fn a_live_lease_of_another_owner_busy_the_scope() {
    let w = world(StoreMode::Conditional);
    pass(&w, "worker-a").run().await.expect("worker A passes");
    let error = pass(&w, "worker-b")
        .run()
        .await
        .expect_err("worker B must be busy");
    assert!(matches!(error, GcError::Busy), "{error:?}");
}

/// The zombie of the expired generation can do NOTHING in the pass:
/// marking, reconfirming, recording, renewing or closing fail with the fence.
#[tokio::test]
async fn a_zombie_with_an_expired_generation_cannot_act() {
    let w = world(StoreMode::Conditional);
    // Generation 1 lives and dies (expired lease, real take-over).
    pass(&w, "worker-a").run().await.expect("worker A passes");
    w.ledger.force_expire();
    pass(&w, "worker-b")
        .run()
        .await
        .expect("worker B takes over");

    let zombie = w.ledger.issued_lease(1);
    let orphan = w.store.put(b"zombie-bait").await.expect("put");
    let run = GcRun {
        id: "gc-zombie".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "zombie".to_string(),
        candidates: vec![RunCandidate {
            digest: orphan.clone(),
            length: 10,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(SCOPE.to_string()),
        generation: Some(1),
    };
    let ledger: &dyn ContentLedger = &*w.ledger;
    assert!(matches!(
        ledger
            .mark_run_fenced(SCOPE, &zombie, &run, std::slice::from_ref(&orphan))
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger.mark_of_run(SCOPE, &zombie, "gc-zombie").await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    let trace = GcDeletion {
        digest: orphan.clone(),
        length: 10,
        run_id: "gc-zombie".to_string(),
        actor: "zombie".to_string(),
        account: SCOPE.to_string(),
        state: DeletionState::Intended,
        at: Utc::now(),
        marker: None,
    };
    assert!(matches!(
        ledger.record_deletion_fenced(SCOPE, &zombie, &trace).await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger
            .renew_gc_lease(
                SCOPE,
                &zombie,
                "zombie",
                Duration::from_secs(30),
                Utc::now()
            )
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger
            .complete_scoped_run(SCOPE, &zombie, "gc-zombie", 0, 0, 0, Utc::now())
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    // And the zombie's blob is still where it was: the new generation did not
    // delete or mark it.
    assert!(w.store.has_key(&orphan));
    assert!(w.ledger.tombstone(&orphan).is_none());
}

/// The pass that loses its lease mid-sweep stops: the
/// remaining candidates are not touched (AC1).
#[tokio::test]
async fn a_pass_that_loses_the_lease_mid_sweep_stops() {
    let w = world(StoreMode::Conditional);
    let first = w.store.put(b"first-orphan").await.expect("put");
    let second = w.store.put(b"second-orphan").await.expect("put");
    w.ledger.fail_renewals_after(2);

    let error = pass(&w, "worker-a")
        .run()
        .await
        .expect_err("the take-over must stop this pass");
    assert!(matches!(error, GcError::Busy), "{error:?}");

    // One of the two was swept completely and the other touched NOTHING: neither
    // intent nor deletion. The census order is not deterministic,
    // so the assertion is on the property, not on which one.
    let first_gone = !w.store.has_key(&first);
    let second_gone = !w.store.has_key(&second);
    assert_ne!(first_gone, second_gone, "exactly one was swept");
    let swept = if first_gone { &first } else { &second };
    let stopped = if first_gone { &second } else { &first };
    assert!(
        matches!(
            w.ledger.tombstone(swept).map(|t| t.state),
            Some(DeletionState::Confirmed)
        ),
        "the swept one has its trace confirmed"
    );
    assert!(
        w.ledger.tombstone(stopped).is_none(),
        "the stopped one was never marked"
    );
}

/// Durable mark + concurrent publisher + resumption: the live reference
/// is NEVER deleted, even if the mark flagged it as an orphan.
#[tokio::test]
async fn a_republished_blob_survives_the_resume_of_an_older_mark() {
    let w = world(StoreMode::Conditional);
    let orphan = w.store.put(b"revived").await.expect("put");

    // A worker of generation 1 marks durably and dies.
    let lease1 = w
        .ledger
        .claim_gc_pass(SCOPE, "worker-a", Duration::from_secs(30), Utc::now())
        .await
        .expect("claim 1");
    let run = GcRun {
        id: "gc-old-mark".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "worker-a".to_string(),
        candidates: vec![RunCandidate {
            digest: orphan.clone(),
            length: 7,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(SCOPE.to_string()),
        generation: Some(1),
    };
    w.ledger
        .mark_run_fenced(SCOPE, &lease1, &run, std::slice::from_ref(&orphan))
        .await
        .expect("durable mark");
    w.ledger.force_expire();

    // A publisher names the content again, and another worker
    // resumes under generation 2.
    w.ledger.add_root(&orphan, "refs/revived");
    let report = pass(&w, "worker-b").run().await.expect("resume pass");
    assert!(report.resumed, "the mark is durable, not in the worker");
    assert_eq!(report.generation, 2);
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Republished)
    );
    assert_eq!(report.deleted, 0);
    assert!(
        w.store.has_key(&orphan),
        "live references are never deleted"
    );
    assert!(
        w.ledger.tombstone(&orphan).is_none(),
        "the intention was never written"
    );
}

// ---------------------------------------------------------------------------
// Conditional delete and AC2.
// ---------------------------------------------------------------------------

/// The happy path: real orphans are deleted conditioned on the
/// marked version, with the trace confirmed post-effect.
#[tokio::test]
async fn a_coordinated_pass_deletes_conditionally_and_confirms_after() {
    let w = world(StoreMode::Conditional);
    let orphan = w.store.put(b"gone-soon").await.expect("put");
    let used = w.store.put(b"used").await.expect("put");
    w.ledger.add_root(&used, "refs/used");

    let report = pass(&w, "worker-a").run().await.expect("pass");
    assert_eq!(report.deleted, 1);
    assert_eq!(report.marked, 1);
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Deleted)
    );
    assert_eq!(report.actions.get(&used.to_string()), None);
    assert!(!w.store.has_key(&orphan));
    let trace = w.ledger.tombstone(&orphan).expect("trace");
    assert_eq!(trace.state, DeletionState::Confirmed);
    assert_eq!(trace.run_id, report.run_id);
    assert!(trace.marker.is_some(), "the version was pinned at intent");
    assert_eq!(report.bytes_freed, 9);
}

/// AC2 (conditional delete): a concurrent rewrite between the
/// intent and the delete produces `NotApplied`. The new key is NOT
/// touched and its reuse is forbidden: the trace stays `Intended`
/// with the old marker, to be reconciled.
#[tokio::test]
async fn a_concurrent_rewrite_blocks_the_delete_and_the_key_is_not_reused() {
    let w = world(StoreMode::Conditional);
    let orphan = w.store.put(b"old-bytes").await.expect("put");
    // When writing the intent, the authority "suffers" the rewrite:
    // another replica uploaded new content under the same key.
    let store = Arc::clone(&w.store);
    let digest = orphan.clone();
    w.ledger.on_record_intention(move || {
        store.republish(&digest, b"new-bytes-rewritten");
    });

    let report = pass(&w, "worker-a").run().await.expect("pass");
    assert_eq!(report.deleted, 0);
    assert_eq!(report.not_applied, 1);
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::NotApplied)
    );
    assert!(w.store.has_key(&orphan), "the new version survives");
    let trace = w.ledger.tombstone(&orphan).expect("trace");
    assert_eq!(
        trace.state,
        DeletionState::Intended,
        "the intent is kept for reconciliation, never confirmed"
    );
}

/// AC2 (versioned quarantine): a store without safe conditional
/// delete does not delete the original key blindly. The marked bytes go
/// to a NEW name, are never rewritten, and the reservation is recorded.
#[tokio::test]
async fn a_store_without_safe_conditional_delete_quarantines_by_version() {
    let w = world(StoreMode::QuarantineOnly);
    let orphan = w.store.put(b"quarantine-me").await.expect("put");

    let report = pass(&w, "worker-a").run().await.expect("pass");
    assert_eq!(report.quarantined, 1);
    assert_eq!(report.deleted, 0, "nothing was deleted from the key");
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Quarantined)
    );
    let record = w.ledger.quarantine_of(&orphan).expect("reservation");
    assert_eq!(record.run_id, report.run_id);
    assert!(record.quarantine_key.starts_with("quarantine/"));
    assert!(
        w.store
            .quarantine_of(&record.quarantine_key)
            .is_some_and(|bytes| bytes == b"quarantine-me"),
        "the marked version lives under the new name"
    );
    assert!(
        !w.store.has_key(&orphan),
        "the original key is freed only after the copy landed"
    );
}

/// AC2/fail-closed: a store with no guard at all (neither version nor
/// condition) gets NOTHING touched. Everything stays `Unknown`.
#[tokio::test]
async fn a_legacy_store_without_guards_is_never_touched() {
    let w = world(StoreMode::Legacy);
    let orphan = w.store.put(b"untouchable").await.expect("put");

    let report = pass(&w, "worker-a").run().await.expect("pass");
    assert_eq!(report.deleted, 0);
    assert_eq!(report.unknown, 1);
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Skipped)
    );
    assert!(w.store.has_key(&orphan));
    assert!(w.ledger.tombstone(&orphan).is_none());
}

// ---------------------------------------------------------------------------
// AC3: paginated census without materializing.
// ---------------------------------------------------------------------------

/// Thousands of roots are walked in bounded pages: the census never
/// materializes the whole graph in one read.
#[tokio::test]
async fn the_census_walks_bounded_pages_without_materializing() {
    let w = world(StoreMode::Conditional);
    let count = 2_000usize;
    for index in 0..count {
        let bytes = format!("blob-{index}");
        let digest = w.store.put(bytes.as_bytes()).await.expect("put");
        w.ledger.add_root(&digest, &format!("refs/{index}"));
    }

    let pass = CoordinatedPass::new(
        Arc::clone(&w.service),
        PassConfig::new(SCOPE, "worker-a", Duration::from_secs(30)).with_page_limit(512),
    );
    let report = pass.run().await.expect("pass");
    // 2000 / 512 = 3 full pages + 1 short final one.
    assert_eq!(report.census_pages, 4);
    assert_eq!(w.ledger.max_page_calls(), 512, "every read is bounded");
    assert_eq!(report.marked, 0, "nothing is orphan: all 2000 are roots");
    assert_eq!(report.deleted, 0);
}

#[tokio::test]
async fn a_restarted_worker_resumes_from_the_durable_mark() {
    let w = world(StoreMode::Conditional);
    let orphan = w.store.put(b"durable").await.expect("put");

    // Generation 1 marks and dies BEFORE deleting.
    let lease1 = w
        .ledger
        .claim_gc_pass(SCOPE, "worker-a", Duration::from_secs(30), Utc::now())
        .await
        .expect("claim");
    let run = GcRun {
        id: "gc-restart".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "worker-a".to_string(),
        candidates: vec![RunCandidate {
            digest: orphan.clone(),
            length: 7,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(SCOPE.to_string()),
        generation: Some(1),
    };
    w.ledger
        .mark_run_fenced(SCOPE, &lease1, &run, std::slice::from_ref(&orphan))
        .await
        .expect("mark");

    // The same worker revives: it renews its own claim (generation 1) and
    // resumes the mark from the ledger.
    w.ledger.force_expire();
    let report = pass(&w, "worker-a").run().await.expect("pass");
    assert!(report.resumed);
    assert_eq!(report.run_id, "gc-restart");
    assert_eq!(report.deleted, 1);
    assert!(!w.store.has_key(&orphan));
}
