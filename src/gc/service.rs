//! The collection service: the census, the dry-run and the sweep.
//!
//! It ties together the three sources --the catalog (roots and retentions), the store
//! (the blobs and their age) and the ledger (the mark and the trail)-- and lets
//! the pure plan decide. Its rules, from most to least important:
//!
//! 1. **When in doubt, nothing is deleted.** If the census or a manifest cannot
//!    be read, the pass aborts: a half-built graph would leave unprotected
//!    what was not seen.
//! 2. **The dry-run is the plan, and the sweep is the same plan.** What
//!    the report says is reclaimable is what the sweep can delete, and
//!    nothing more.
//! 3. **Mark and sweep are durable and idempotent.** The pass is
//!    written before a byte is touched; a cut in between is resumed
//!    without deleting twice and without losing what was marked.
//! 4. **Deletion leaves a trail.** The intent is recorded before deleting
//!    and confirmed afterwards; the audit event is emitted on confirmation.
//! 5. **Only one pass at a time**, and each deletion re-checks the
//!    age under the store's barrier.
//! 6. **The dry-run shares the census.** A snapshot of the store is worth the same
//!    for twenty requests as for one, and computing it twenty times at
//!    once is not fresher: it is twenty complete graphs piled up in
//!    memory and on disk. What deletes --the sweep and the scrub-- always
//!    takes its own census, on the spot.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcRun,
    HoldScope, RootKind, RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobError, BlobReclaim, BlobStore, ReadPolicy, Reclaimed, StoredBlob};
use serde::Serialize;
use tokio::sync::Mutex;

use super::pass::{CoordinatedPass, PassConfig, ROOT_PAGE_LIMIT};
use super::plan::{self, EdgeError, Input, Plan, Verdict};

/// The scope under which the platform-wide sweep of a fenced ledger claims
/// its pass: the content store is shared by every account, so there is one
/// collection per deployment, not one per account.
const PLATFORM_SCOPE: &str = "platform";

/// How long the claim of a platform sweep lives between renewals. The pass
/// renews it before every candidate and releases it when it ends.
const SWEEP_LEASE: Duration = Duration::from_secs(60);

/// Why a collection was not carried out.
#[derive(Debug, thiserror::Error)]
pub enum GcError {
    /// The store cannot list or delete with a barrier.
    #[error("the content store does not support reclamation")]
    Unsupported,
    /// A pass is already in progress.
    #[error("a collection is already running")]
    Busy,
    /// It could not be determined what is alive, and so nothing is touched.
    #[error("the state of the content is unknown, nothing was deleted: {0}")]
    Unknown(String),
    /// The catalog or the ledger failed.
    #[error("the catalog failed: {0}")]
    Ledger(#[from] CatalogError),
    /// The store failed.
    #[error("the content store failed: {0}")]
    Store(String),
    /// The request is not valid.
    #[error("{0}")]
    Invalid(String),
}

/// The result of a sweep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SweepReport {
    /// The pass.
    pub run_id: String,
    /// Whether it is a pass marked earlier and resumed now.
    pub resumed: bool,
    /// How many blobs were marked.
    pub marked: u64,
    /// How many were deleted in this pass.
    pub deleted: u64,
    /// The bytes freed.
    pub bytes_freed: u64,
    /// How many were left undeleted because they stopped being reclaimable.
    pub skipped: u64,
}

/// How the census treats a manifest that cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestReads {
    /// An unreadable manifest is unknown state: collection does not act.
    Strict,
    /// A missing or corrupt manifest contributes no edges and the census continues.
    Lenient,
}

/// The maximum validity window of the dry-run's shared plan: a
/// plan that is thirty seconds old is an informed plan, not a
/// broken one; a plan that is hours old is not a plan.
const PLAN_CACHE_MAX_TTL: Duration = Duration::from_secs(30);

/// The last dry-run plan: the generation it was computed with, the
/// instant it was published and the shared plan.
type CachedPlan = (u64, Instant, Arc<Plan>);

/// Garbage collection of the store.
pub struct GcService {
    ledger: Arc<dyn ContentLedger>,
    store: Arc<dyn BlobStore>,
    grace: Duration,
    /// Only one pass at a time: two concurrent sweeps would write
    /// two marks and trample each other's trail.
    gate: Mutex<()>,
    /// How long a dry-run plan is worth: whatever the grace period says, capped
    /// at the maximum. With a grace of milliseconds the plan cannot be
    /// worth more than it, because it ages inside its own window.
    plan_ttl: Duration,
    /// The shared plan. It is read, written and dropped without holding the
    /// lock across an `await`: what a census takes is held
    /// with [`Self::plan_flight`], and here it is only looked at and published.
    plan_cache: Mutex<Option<CachedPlan>>,
    /// The single flight: whoever computes a census holds it; the others
    /// wait behind it here and enter the cache to pick up the result.
    plan_flight: Mutex<()>,
    /// The generation of the world. Every mutation that changes what the plan
    /// would say --retentions, sweeps-- breaks it, and a plan from a
    /// dead generation is never served.
    plan_generation: AtomicU64,
    /// Who this process is for the claim of a coordinated pass. Unique per
    /// process: two replicas exclude each other through the ledger, and a
    /// restarted process never mistakes itself for its dead predecessor.
    pass_owner: String,
}

impl GcService {
    /// Creates the service. Fails if the store cannot reclaim.
    pub fn new(
        ledger: Arc<dyn ContentLedger>,
        store: Arc<dyn BlobStore>,
        grace: Duration,
    ) -> Result<Self, GcError> {
        if store.reclaimer().is_none() {
            return Err(GcError::Unsupported);
        }
        Ok(Self {
            ledger,
            store,
            grace,
            gate: Mutex::new(()),
            plan_ttl: grace.min(PLAN_CACHE_MAX_TTL),
            plan_cache: Mutex::new(None),
            plan_flight: Mutex::new(()),
            plan_generation: AtomicU64::new(0),
            pass_owner: format!("gc-{}", uuid::Uuid::new_v4().simple()),
        })
    }

    /// The grace period.
    pub fn grace(&self) -> Duration {
        self.grace
    }

    /// The durable ledger behind the service (for the coordinated pass).
    pub(crate) fn ledger(&self) -> &Arc<dyn ContentLedger> {
        &self.ledger
    }

    /// The content store behind the service.
    pub(crate) fn store(&self) -> &Arc<dyn BlobStore> {
        &self.store
    }

    /// The single-pass gate, shared by the legacy sweep and the
    /// coordinated pass: one writer per process, whatever the mode.
    pub(crate) fn gate(&self) -> &Mutex<()> {
        &self.gate
    }

    fn reclaimer(&self) -> Result<&dyn BlobReclaim, GcError> {
        self.store.reclaimer().ok_or(GcError::Unsupported)
    }

    /// The plan as of now: the dry-run. It neither writes nor deletes anything.
    ///
    /// The plan is shared within a freshness window and computed by one
    /// census at a time: concurrent callers wait on the census instead of
    /// piling full graphs into memory and disk. A result computed while
    /// the world changed (the generation moved under it) is returned but
    /// never cached. Sweep and scrub never come through here: what
    /// deletes always takes its own fresh census.
    pub async fn dry_run(&self) -> Result<Arc<Plan>, GcError> {
        // Lock order, always the same here and in the invalidation:
        // flight first, cache second. Nothing else ever takes the flight.
        let _flight = self.plan_flight.lock().await;
        let generation = self.plan_generation.load(Ordering::Acquire);
        let cached = self.plan_cache.lock().await.clone();
        if let Some((cached_generation, at, plan)) = cached
            && cached_generation == generation
            && at.elapsed() < self.plan_ttl
        {
            return Ok(plan);
        }
        let (plan, _) = self.census().await?;
        let plan = Arc::new(plan);
        let mut cache = self.plan_cache.lock().await;
        if self.plan_generation.load(Ordering::Acquire) == generation {
            *cache = Some((generation, Instant::now(), plan.clone()));
        }
        Ok(plan)
    }

    /// Drops the shared plan: the next dry-run computes a fresh census.
    ///
    /// Called after anything that changes what the plan would say. When
    /// a census is in flight the generation moves anyway, so the in-flight
    /// result is returned but not cached.
    pub(crate) fn invalidate_plan_cache(&self) {
        self.plan_generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut cache) = self.plan_cache.try_lock() {
            *cache = None;
        }
    }

    /// The census and the plan that comes out of it.
    ///
    /// It also returns the blobs, because the sweep needs to know what
    /// is still in the store.
    pub(crate) async fn census(&self) -> Result<(Plan, Vec<StoredBlob>), GcError> {
        self.census_with(ManifestReads::Strict).await
    }

    /// Like [`Self::census`], with the manifest-reading policy.
    ///
    /// `Lenient` is for the scrub: a missing or corrupt manifest does not
    /// abort the census, because that is exactly what the scrub has to
    /// report. Other read failures remain unknown state.
    pub(crate) async fn census_with(
        &self,
        reads: ManifestReads,
    ) -> Result<(Plan, Vec<StoredBlob>), GcError> {
        let blobs = self.reclaimer()?.list_blobs().await.map_err(|error| {
            GcError::Unknown(format!("the store could not be listed: {error:?}"))
        })?;
        let roots = self.census_roots().await?;
        let holds =
            self.ledger.holds().await.map_err(|error| {
                GcError::Unknown(format!("the holds could not be read: {error}"))
            })?;

        // The edges: what each live manifest names. They are read from
        // the manifest bytes, which is where the truth lives, and are not
        // persisted: a separate index would be a second truth.
        let mut edges: HashMap<ContentDigest, Vec<ContentDigest>> = HashMap::new();
        let mut seen: HashSet<ContentDigest> = HashSet::new();
        for root in &roots {
            // An OCI manifest is the asset `manifests/<its digest>`. Any raw
            // path that starts with `manifests/` is not one,
            // and cannot leave collection in unknown state.
            if root.kind != RootKind::Asset
                || root.subject.strip_prefix("manifests/") != Some(root.digest.value.as_str())
            {
                continue;
            }
            if !seen.insert(root.digest.clone()) {
                continue;
            }
            let bytes = match self.store.get_with(&root.digest, ReadPolicy::Verify).await {
                Ok(bytes) => bytes,
                Err(error @ (BlobError::NotFound | BlobError::Corrupted { .. }))
                    if reads == ManifestReads::Lenient =>
                {
                    tracing::warn!(
                        digest = %root.digest,
                        ?error,
                        "a manifest is missing or damaged: it contributes no edges"
                    );
                    continue;
                }
                Err(error) => {
                    return Err(GcError::Unknown(format!(
                        "the manifest {} could not be read: {error:?}",
                        root.digest
                    )));
                }
            };
            // The edges are defense in depth: each layer and each
            // config already has its own `blobs/<digest>` root, and
            // each child of an index is a manifest with its own. A
            // manifest that is not understood (a raw file with that
            // path, a type the registry accepts and the census does not know)
            // contributes no edges, but neither does it block collection of
            // the whole store: its own content stays protected by the
            // root that names it.
            match plan::manifest_edges(&bytes) {
                Ok(children) => {
                    edges.insert(root.digest.clone(), children);
                }
                Err(error @ EdgeError::NotJson)
                | Err(error @ EdgeError::Unrecognized)
                | Err(error @ EdgeError::BadDigest) => {
                    tracing::warn!(
                        digest = %root.digest,
                        %error,
                        "a manifest is not understood: it contributes no edges"
                    );
                }
            }
        }

        let plan = plan::plan(&Input {
            blobs: &blobs,
            roots: &roots,
            holds: &holds,
            edges: &edges,
            grace: self.grace,
            now: Utc::now(),
            roots_are_complete: true,
        });
        Ok((plan, blobs))
    }

    /// Every root of the catalog, read the way the ledger can give them.
    ///
    /// A ledger that fences a coordinated pass has no global census (it
    /// would materialise the whole graph): the roots come page by page,
    /// account by account. Any other ledger answers with one read. A
    /// failure is an unknown state, never a partial census.
    pub(crate) async fn census_roots(&self) -> Result<Vec<ContentRoot>, GcError> {
        let unknown = |error: &dyn std::fmt::Display| {
            GcError::Unknown(format!("the roots could not be read: {error}"))
        };
        if !self.ledger.supports_fenced_pass() {
            return self.ledger.content_roots().await.map_err(|e| unknown(&e));
        }
        self.platform_roots(ROOT_PAGE_LIMIT)
            .await
            .map(|(roots, _)| roots)
            .map_err(|error| match error {
                GcError::Ledger(error) => unknown(&error),
                other => other,
            })
    }

    /// The roots of every account, in pages of `page_limit`. Returns the
    /// roots and how many pages were read.
    pub(crate) async fn platform_roots(
        &self,
        page_limit: usize,
    ) -> Result<(Vec<ContentRoot>, u64), GcError> {
        let mut roots = Vec::new();
        let mut pages = 0u64;
        for account in self.ledger.census_accounts().await? {
            let (account_roots, account_pages) =
                scope_roots(&self.ledger, &account, page_limit).await?;
            roots.extend(account_roots);
            pages += account_pages;
        }
        Ok((roots, pages))
    }

    /// The sweep the API runs: the mode that fits the ledger.
    ///
    /// A ledger that fences gets one coordinated pass over the whole
    /// platform, claimed durably, so the claim and the pending mark are
    /// read by key and never by a global scan. Any other ledger gets the
    /// single-process [`Self::sweep`].
    pub async fn sweep_platform(
        self: &Arc<Self>,
        actor: &str,
        account: &str,
    ) -> Result<SweepReport, GcError> {
        if self.ledger.supports_fenced_pass() {
            self.sweep_fenced(actor).await
        } else {
            self.sweep(actor, account).await
        }
    }

    async fn sweep_fenced(self: &Arc<Self>, actor: &str) -> Result<SweepReport, GcError> {
        let config = PassConfig::new(PLATFORM_SCOPE, self.pass_owner.clone(), SWEEP_LEASE)
            .across_accounts()
            .on_behalf_of(actor);
        let report = CoordinatedPass::new(Arc::clone(self), config).run().await?;
        let removed = report.deleted + report.quarantined;
        Ok(SweepReport {
            run_id: report.run_id,
            resumed: report.resumed,
            marked: report.marked,
            deleted: report.deleted,
            bytes_freed: report.bytes_freed,
            skipped: report.marked.saturating_sub(removed),
        })
    }

    /// Sweeps: marks what is reclaimable and deletes it.
    ///
    /// If there is a marked pass that was not completed --a cut between the mark
    /// and the sweep--, it **resumes** it instead of opening another.
    pub async fn sweep(&self, actor: &str, account: &str) -> Result<SweepReport, GcError> {
        let _single = self.gate.try_lock().map_err(|_| GcError::Busy)?;
        let reclaimer = self.reclaimer()?;

        let pending = self.ledger.pending_run().await?;
        let resumed = pending.is_some();
        let run = match pending {
            Some(run) => run,
            None => {
                // First scan: what gets marked.
                let (plan, _) = self.census().await?;
                let candidates: Vec<RunCandidate> = plan
                    .reclaimable()
                    .filter_map(|entry| {
                        parse_digest(&entry.digest).map(|digest| RunCandidate {
                            digest,
                            length: entry.length,
                        })
                    })
                    .collect();
                let run = GcRun {
                    id: format!("gc-{}", uuid::Uuid::new_v4().simple()),
                    state: RunState::Marked,
                    grace_secs: self.grace.as_secs(),
                    actor: actor.to_string(),
                    candidates,
                    marked_at: Utc::now(),
                    deleted: 0,
                    bytes_freed: 0,
                    swept_at: None,
                    // Legacy mode: no scope or claim generation.
                    scope: None,
                    generation: None,
                };
                // The mark is written before a byte is touched.
                self.ledger.open_run(&run).await?;
                run
            }
        };

        // Second scan: what was marked is re-evaluated with a fresh census.
        // Something may have been published, retained or become younger since the mark;
        // only what is still reclaimable now is deleted.
        let (fresh, blobs) = self.census().await?;
        let reclaimable_now: HashSet<String> = fresh
            .entries
            .iter()
            .filter(|entry| matches!(entry.verdict, Verdict::Reclaimable { .. }))
            .map(|entry| entry.digest.clone())
            .collect();
        let present: HashSet<String> = blobs.iter().map(|blob| blob.digest.to_string()).collect();

        let mut skipped = 0u64;
        for candidate in &run.candidates {
            let key = candidate.digest.to_string();
            let trace = self.ledger.deletion_of(&candidate.digest).await?;
            let mine = trace.filter(|trace| trace.run_id == run.id);

            match mine {
                // Already confirmed by this pass: not repeated.
                Some(trace) if trace.state == DeletionState::Confirmed => continue,
                // The intent was recorded and the blob is no longer there: the cut
                // was between the deletion and the confirmation.
                Some(trace) if !present.contains(&key) => {
                    self.confirm(&trace, candidate.length, account).await?;
                    continue;
                }
                _ => {}
            }

            if !reclaimable_now.contains(&key) {
                skipped += 1;
                continue;
            }

            let intended = GcDeletion {
                digest: candidate.digest.clone(),
                length: candidate.length,
                run_id: run.id.clone(),
                actor: actor.to_string(),
                account: account.to_string(),
                state: DeletionState::Intended,
                at: Utc::now(),
                // Legacy mode: the sweep does not observe blob version.
                marker: None,
            };
            self.ledger.record_deletion(&intended).await?;

            match reclaimer
                .delete_if_older_than(&candidate.digest, self.grace)
                .await
            {
                Ok(Reclaimed::Deleted { length }) => {
                    self.confirm(&intended, length, account).await?;
                }
                Ok(Reclaimed::Gone) => {
                    // It went away between the census and the deletion: we do not delete it
                    // now, and it is not counted as our deletion.
                    skipped += 1;
                }
                Ok(Reclaimed::Fresh) => skipped += 1,
                Err(error) => {
                    return Err(GcError::Store(format!(
                        "{} could not be deleted: {error:?}",
                        candidate.digest
                    )));
                }
            }
        }

        // The totals come from the durable trail, not from this execution:
        // a resumed pass counts what it deleted before the cut.
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        for candidate in &run.candidates {
            if let Some(trace) = self.ledger.deletion_of(&candidate.digest).await?
                && trace.run_id == run.id
                && trace.state == DeletionState::Confirmed
            {
                deleted += 1;
                bytes_freed += trace.length;
            }
        }
        self.ledger
            .complete_run(&run.id, deleted, bytes_freed, Utc::now())
            .await?;
        // The world changed bytes: the shared plan of the dry-run is
        // stale from here on, whatever this pass deleted.
        self.invalidate_plan_cache();

        Ok(SweepReport {
            run_id: run.id,
            resumed,
            marked: run.candidates.len() as u64,
            deleted,
            bytes_freed,
            skipped,
        })
    }

    async fn confirm(
        &self,
        intended: &GcDeletion,
        length: u64,
        account: &str,
    ) -> Result<(), GcError> {
        let confirmed = GcDeletion {
            state: DeletionState::Confirmed,
            length,
            account: account.to_string(),
            at: Utc::now(),
            ..intended.clone()
        };
        self.ledger.record_deletion(&confirmed).await?;
        Ok(())
    }

    /// The registered retentions.
    pub async fn holds(&self) -> Result<Vec<ContentHold>, GcError> {
        Ok(self.ledger.holds().await?)
    }

    /// Places a retention.
    pub async fn place_hold(
        &self,
        actor: &str,
        scope: HoldScope,
        reason: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<ContentHold, GcError> {
        if reason.trim().is_empty() {
            return Err(GcError::Invalid("a hold needs a reason".to_string()));
        }
        if expires_at.is_some_and(|at| at <= Utc::now()) {
            return Err(GcError::Invalid(
                "a hold cannot expire in the past".to_string(),
            ));
        }
        let hold = ContentHold {
            id: format!("hold-{}", uuid::Uuid::new_v4().simple()),
            scope,
            reason: reason.trim().to_string(),
            actor: actor.to_string(),
            created_at: Utc::now(),
            expires_at,
        };
        self.ledger.place_hold(&hold).await?;
        self.invalidate_plan_cache();
        Ok(hold)
    }

    /// Places retentions with caller-chosen, deterministic identifiers.
    ///
    /// Used by the dependency freeze (HSP-161): the identifier names the
    /// freeze that owns the retention, and placing the same one again replaces
    /// it, so a repeated freeze is idempotent. The plan cache is dropped: what
    /// the plan would say has changed.
    pub async fn place_holds(&self, holds: &[ContentHold]) -> Result<(), GcError> {
        let mut failure = None;
        for hold in holds {
            if let Err(error) = self.ledger.place_hold(hold).await {
                failure = Some(error);
                break;
            }
        }
        self.invalidate_plan_cache();
        failure.map_or(Ok(()), |error| Err(error.into()))
    }

    /// One retention by identifier (a point read, in force or not).
    pub async fn hold(&self, id: &str) -> Result<Option<ContentHold>, GcError> {
        Ok(self.ledger.hold(id).await?)
    }

    /// Releases every retention whose identifier starts with `prefix` and
    /// returns how many existed. Only what the prefix names is touched: the
    /// retentions of other owners are left exactly as they are.
    ///
    /// The retention named `last` is released **after** all the others, and
    /// only if all the others were released: it is the registry of its owner,
    /// so a release that fails half way leaves the owner findable and the
    /// release can be repeated.
    pub async fn release_holds_with_prefix(
        &self,
        prefix: &str,
        last: &str,
    ) -> Result<usize, GcError> {
        let mut released = 0;
        let mut failure = None;
        let mut last_exists = false;
        for hold in self.ledger.holds().await? {
            if !hold.id.starts_with(prefix) {
                continue;
            }
            if hold.id == last {
                last_exists = true;
                continue;
            }
            match self.ledger.release_hold(&hold.id).await {
                Ok(true) => released += 1,
                Ok(false) => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if failure.is_none() && last_exists {
            match self.ledger.release_hold(last).await {
                Ok(true) => released += 1,
                Ok(false) => {}
                Err(error) => failure = Some(error),
            }
        }
        self.invalidate_plan_cache();
        failure.map_or(Ok(released), |error| Err(error.into()))
    }

    /// Releases a retention. Returns whether it existed.
    pub async fn release_hold(&self, id: &str) -> Result<bool, GcError> {
        let released = self.ledger.release_hold(id).await?;
        if released {
            self.invalidate_plan_cache();
        }
        Ok(released)
    }
}

/// Pages the roots of one scope until the census is complete. Returns the
/// roots and how many pages were read.
pub(crate) async fn scope_roots(
    ledger: &Arc<dyn ContentLedger>,
    scope: &str,
    page_limit: usize,
) -> Result<(Vec<ContentRoot>, u64), GcError> {
    let mut roots = Vec::new();
    let mut pages = 0u64;
    let mut cursor: Option<String> = None;
    loop {
        let page = ledger
            .root_page(scope, cursor.as_deref(), page_limit)
            .await?;
        pages += 1;
        let last = page.roots.last().map(|root| root.digest.to_string());
        let has_more = page.has_more;
        roots.extend(page.roots);
        if !has_more {
            return Ok((roots, pages));
        }
        cursor =
            Some(last.ok_or_else(|| {
                GcError::Unknown("a full root page carried no cursor".to_string())
            })?);
    }
}

pub(crate) fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    Some(ContentDigest::sha256(value))
}

impl From<BlobError> for GcError {
    fn from(error: BlobError) -> Self {
        Self::Store(format!("{error:?}"))
    }
}
