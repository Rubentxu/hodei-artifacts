//! The coordinated, fenced pass across replicas (HSP-192).
//!
//! The service's `sweep` is the legacy mode: one process, one in-memory
//! lock. The coordinated pass is the cross-replica mode, and that is why
//! none of its state lives in the worker's memory:
//!
//! 1. **The claim is durable.** It is taken with `claim_gc_pass` over the
//!    scope; it expires by lease; every later operation presents the
//!    token of its generation. A zombie worker with an expired
//!    generation cannot mark, confirm or close: the authority
//!    rejects it. The success of the claim is decided by **re-reading after
//!    committing** (HSP-205 rule), not because the claim transaction
//!    committed.
//! 2. **The mark is transactional with guards.** `mark_run_fenced`
//!    writes the pass verifying the current claim and the live
//!    references in the SAME transaction (frontier pattern,
//!    HSP-199) and returns what was marked, read post-commit.
//! 3. **Before touching a byte, revalidate.** `live_reference_count`
//!    against the real state: if a publisher named the blob, it is
//!    discarded and its intent is not confirmed. After the effect, the
//!    verdict comes from re-reading the store (confirmation only
//!    post-applied), and an orphaned intent from a cut is
//!    reconciled by re-reading: attempted + blob gone = confirmed.
//! 4. **The object store decides how it is removed (AC2).** With conditional
//!    delete, the deletion is conditioned on the marked version:
//!    a concurrent rewrite produces `NotApplied` and the key is NOT
//!    reused until reconciled. With version capability but
//!    no conditional delete, versioned quarantine: the marked
//!    bytes go to a NEW name (never a rewrite) and the key
//!    stays reserved in the ledger. With neither, the pass
//!    **deletes nothing** and everything stays `Unknown`. The question is
//!    answered by the store's type, not by a configuration.
//! 5. **The census goes by pages (AC3).** `root_page` walks the
//!    roots bounded by `page_limit` without materializing the graph. While
//!    the census has not finished, `roots_are_complete` is false and the pure
//!    plan degrades the orphaned to `Unknown`: **Unknown never deletes**.
//! 6. **Resumption comes from the ledger.** A cut between mark and
//!    sweep is recovered with `pending_scoped_run` + `mark_of_run`:
//!    the durable trail, not memory.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{
    CatalogError, ContentLedger, DeletionState, GcDeletion, GcLease, GcRun, Quarantined,
    RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{ConditionalBlob, ConditionalOutcome, VersionMarker};
use serde::Serialize;

use super::plan::{self, Input, Plan};
use super::service::{GcError, GcService};

/// How many roots fit in one page of the coordinated census.
///
/// It is the materialization bound: the census never piles the whole graph
/// in memory. In a deployment it is configured; here it is
/// a constant so the test can exercise it.
pub const ROOT_PAGE_LIMIT: usize = 512;

/// The knobs of a coordinated pass.
#[derive(Debug, Clone)]
pub struct PassConfig {
    /// The authority scope under which it is coordinated (e.g. the HRN of
    /// the deployment's single account). Two workers with the same scope
    /// exclude each other through the claim; with different scopes they do not see each other, which
    /// is how "across replicas" is shown over a single authority.
    pub scope: String,
    /// Who this worker is. A live claim held by someone else occupies the scope
    /// (Busy); an expired claim is taken over with a new generation.
    pub owner: String,
    /// How long the claim is worth. It is renewed between marks.
    pub lease_for: Duration,
    /// The page bound of the census.
    pub page_limit: usize,
    /// Whether the census spans every account of the deployment (the
    /// platform-wide sweep) instead of the one account the scope names.
    pub all_accounts: bool,
    /// The person on whose behalf the pass runs, recorded as the actor of
    /// the run and of its deletions. Defaults to the owner.
    pub actor: Option<String>,
}

impl PassConfig {
    /// The minimal constructor: scope, owner and lease; default page.
    pub fn new(scope: impl Into<String>, owner: impl Into<String>, lease_for: Duration) -> Self {
        Self {
            scope: scope.into(),
            owner: owner.into(),
            lease_for,
            page_limit: ROOT_PAGE_LIMIT,
            all_accounts: false,
            actor: None,
        }
    }

    /// The census spans every account of the deployment: the content store
    /// is shared, so a root of any account keeps a blob alive.
    pub fn across_accounts(mut self) -> Self {
        self.all_accounts = true;
        self
    }

    /// Records `actor` as who asked for the pass.
    pub fn on_behalf_of(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    fn actor(&self) -> &str {
        self.actor.as_deref().unwrap_or(&self.owner)
    }

    /// Changes the page bound (tests lower it to force the
    /// paginated walk with hundreds of roots).
    pub fn with_page_limit(mut self, page_limit: usize) -> Self {
        self.page_limit = page_limit.max(1);
        self
    }
}

/// What the pass did to a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassAction {
    /// The blob was deleted conditioned on the marked version.
    Deleted,
    /// The bytes were moved to versioned quarantine (AC2, stores
    /// without conditional delete).
    Quarantined,
    /// Something names the content: it was discarded without touching it.
    Republished,
    /// The version changed since the mark: **the key was not reused**
    /// until reconciled (AC2).
    NotApplied,
    /// The blob was no longer there: another pass acted on it first.
    Gone,
    /// No guard applies: `Unknown`, never deleted.
    Skipped,
}

/// The result of a coordinated pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassReport {
    /// The pass.
    pub run_id: String,
    /// The generation of the claim under which it sweeps.
    pub generation: u64,
    /// Whether the pass resumed a pending mark.
    pub resumed: bool,
    /// How many candidates were marked.
    pub marked: u64,
    /// How many were deleted (including those confirmed by re-reading).
    pub deleted: u64,
    /// The bytes freed.
    pub bytes_freed: u64,
    /// How many ended up in versioned quarantine.
    pub quarantined: u64,
    /// How many were discarded because something named them (revalidation).
    pub republished: u64,
    /// How many were not touched because their version changed (AC2).
    pub not_applied: u64,
    /// How many ended up as `Unknown`: they were never deleted.
    pub unknown: u64,
    /// How many pages of roots were read (AC3: the visible bound).
    /// Zero if the pass resumed and needed no new census.
    pub census_pages: u64,
    /// Action per digest, for membership assertions.
    pub actions: HashMap<String, PassAction>,
}

/// How the store allows removing the marked version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreMode {
    /// Delete conditioned on the version: the store
    /// closes the race. `NotApplied` forbids key reuse.
    Conditional,
    /// Versioned quarantine only: new name, never a rewrite.
    Quarantine,
    /// Neither: the coordinated pass touches nothing.
    Refuse,
}

/// The coordinated pass worker.
///
/// It lives without state between calls: everything it needs to resume
/// is in the ledger. Two instances over the same scope compete for
/// the claim; the authority decides.
pub struct CoordinatedPass {
    service: Arc<GcService>,
    config: PassConfig,
}

impl CoordinatedPass {
    /// Creates the worker over the existing service.
    pub fn new(service: Arc<GcService>, config: PassConfig) -> Self {
        Self { service, config }
    }

    /// The ledger behind the pass.
    fn ledger(&self) -> &Arc<dyn ContentLedger> {
        self.service.ledger()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// The fenced claim: `claim_gc_pass` + post-commit re-read
    /// (HSP-205 rule: what the authority says on reading rules, not the
    /// fact that the claim committed).
    async fn claim(&self) -> Result<GcLease, GcError> {
        if !self.ledger().supports_fenced_pass() {
            return Err(GcError::Unsupported);
        }
        let lease = self
            .ledger()
            .claim_gc_pass(
                &self.config.scope,
                &self.config.owner,
                self.config.lease_for,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)?;
        let back = self
            .ledger()
            .current_gc_lease(&self.config.scope, Self::now())
            .await?
            .ok_or_else(|| {
                GcError::Unknown("the claim was not readable after commit".to_string())
            })?;
        if back.token != lease.token || back.generation != lease.generation {
            // Another replica won the claim race: we are NOT
            // the holder and we do not sweep.
            return Err(GcError::Busy);
        }
        Ok(lease)
    }

    /// The claim error: if the scope is occupied or the CAS condition
    /// was not met, it is Busy; anything else comes from the ledger.
    fn busy_or_ledger(error: CatalogError) -> GcError {
        match error {
            CatalogError::Conflict { .. } | CatalogError::PreconditionFailed { .. } => {
                GcError::Busy
            }
            other => GcError::Ledger(other),
        }
    }

    /// The pending mark under the current claim, read from the
    /// authority. `None` if there is no pass to resume.
    async fn pending_mark(&self, lease: &GcLease) -> Result<Option<GcRun>, GcError> {
        let Some(run) = self
            .ledger()
            .pending_scoped_run(&self.config.scope, Self::now())
            .await?
        else {
            return Ok(None);
        };
        if run.scope.as_deref() != Some(self.config.scope.as_str()) {
            return Err(GcError::Unknown(format!(
                "the pending run {} is not of this scope",
                run.id
            )));
        }
        // The fenced reconfirmation: if the trail no longer exists under the
        // current generation (the authority vetoed it), it is not resumed; a
        // new mark is opened.
        Ok(self
            .ledger()
            .mark_of_run(&self.config.scope, lease, &run.id)
            .await?)
    }

    /// The paginated census (AC3): walks `root_page` bounded by
    /// `page_limit` without materializing the graph and produces the plan with the
    /// same pure core as the dry-run.
    ///
    /// `roots_are_complete` is only true when the walk finished
    /// without `has_more`: with a half-done census nothing is reclaimable -- the
    /// verdict is `Unknown`, which is never deleted.
    pub async fn paged_plan(&self) -> Result<(Plan, u64), GcError> {
        let store = Arc::clone(self.service.store());
        let blobs = store
            .reclaimer()
            .ok_or(GcError::Unsupported)?
            .list_blobs()
            .await
            .map_err(|error| {
                GcError::Unknown(format!("the store could not be listed: {error:?}"))
            })?;
        let holds =
            self.ledger().holds().await.map_err(|error| {
                GcError::Unknown(format!("the holds could not be read: {error}"))
            })?;

        let (roots, pages) = if self.config.all_accounts {
            self.service.platform_roots(self.config.page_limit).await?
        } else {
            super::service::scope_roots(self.ledger(), &self.config.scope, self.config.page_limit)
                .await?
        };
        // Reaching here means every page was read: a failure above aborts.
        let complete = true;

        // The manifest edges, over the already paginated roots.
        let mut edges: HashMap<ContentDigest, Vec<ContentDigest>> = HashMap::new();
        let mut seen: HashSet<ContentDigest> = HashSet::new();
        for root in &roots {
            if root.kind != hodei_catalog::RootKind::Asset
                || root.subject.strip_prefix("manifests/") != Some(root.digest.value.as_str())
            {
                continue;
            }
            if !seen.insert(root.digest.clone()) {
                continue;
            }
            let bytes = match store
                .get_with(&root.digest, hodei_hub::ReadPolicy::Verify)
                .await
            {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Err(GcError::Unknown(format!(
                        "the manifest {} could not be read: {error:?}",
                        root.digest
                    )));
                }
            };
            if let Ok(children) = plan::manifest_edges(&bytes) {
                edges.insert(root.digest.clone(), children);
            }
        }

        let plan = plan::plan(&Input {
            blobs: &blobs,
            roots: &roots,
            holds: &holds,
            edges: &edges,
            grace: self.service.grace(),
            now: Self::now(),
            roots_are_complete: complete,
        });
        Ok((plan, pages))
    }

    /// A full pass: claim -> durable mark -> revalidation ->
    /// destination according to the store's mode -> fenced close.
    ///
    /// If the lease is lost midway (another replica took the scope),
    /// the pass stops with `GcError::Busy` and not one more mark: the
    /// zombie does not keep deleting the new generation (AC1).
    pub async fn run(&self) -> Result<PassReport, GcError> {
        let _single = self.service.gate().try_lock().map_err(|_| GcError::Busy)?;
        let mut lease = self.claim().await?;

        // 1. Durable resumption or new fenced mark.
        let pending = self.pending_mark(&lease).await?;
        let resumed = pending.is_some();
        let (run, census_pages, plan_unknown) = match pending {
            Some(run) => (run, 0u64, 0u64),
            None => {
                let (plan, pages) = self.paged_plan().await?;
                let candidates: Vec<RunCandidate> = plan
                    .reclaimable()
                    .filter_map(|entry| {
                        super::service::parse_digest(&entry.digest).map(|digest| RunCandidate {
                            digest,
                            length: entry.length,
                        })
                    })
                    .collect();
                let draft = GcRun {
                    id: format!("gc-{}", uuid::Uuid::new_v4().simple()),
                    state: RunState::Marked,
                    grace_secs: self.service.grace().as_secs(),
                    actor: self.config.actor().to_string(),
                    candidates,
                    marked_at: Self::now(),
                    deleted: 0,
                    bytes_freed: 0,
                    swept_at: None,
                    scope: Some(self.config.scope.clone()),
                    generation: Some(lease.generation),
                };
                // The mark goes with guards in the SAME transaction;
                // what is swept is what the authority returned
                // read post-commit, not what the plan imagined.
                let marked = self
                    .ledger()
                    .mark_run_fenced(
                        &self.config.scope,
                        &lease,
                        &draft,
                        &draft
                            .candidates
                            .iter()
                            .map(|candidate| candidate.digest.clone())
                            .collect::<Vec<ContentDigest>>(),
                    )
                    .await
                    .map_err(Self::busy_or_ledger)?;
                (
                    GcRun {
                        candidates: marked,
                        ..draft
                    },
                    pages,
                    plan.summary.unknown,
                )
            }
        };

        // 2. Sweep with per-candidate revalidation.
        let store = Arc::clone(self.service.store());
        let mode = match store.conditional() {
            Some(blob) if blob.supports_conditional_delete() => StoreMode::Conditional,
            Some(_) => StoreMode::Quarantine,
            None => StoreMode::Refuse,
        };

        let mut actions: HashMap<String, PassAction> = HashMap::new();
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        let mut quarantined = 0u64;
        let mut not_applied = 0u64;
        let mut unknown = plan_unknown;
        let mut republished = 0u64;

        for candidate in &run.candidates {
            // The lease is renewed on every candidate: a worker whose
            // claim passed to another generation stops sweeping instantly.
            lease = self.renew(&lease).await?;

            let key = candidate.digest.to_string();

            // The own durable trail first: an intent of this
            // run may predate the cut and is resolved ONLY by
            // re-reading the real state.
            if let Some(trace) = self.ledger().deletion_of(&candidate.digest).await?
                && trace.run_id == run.id
            {
                match trace.state {
                    DeletionState::Confirmed => {
                        deleted += 1;
                        bytes_freed += trace.length;
                        actions.insert(key, PassAction::Deleted);
                        continue;
                    }
                    DeletionState::Intended => {
                        let vanished = if mode == StoreMode::Refuse {
                            // Without conditional capability no own intents are
                            // written; if they arrive from a replaced
                            // store, it is decided by `exists`.
                            !store
                                .exists(&candidate.digest)
                                .await
                                .map_err(|error| GcError::Store(format!("{error:?}")))?
                        } else {
                            self.version_of(&candidate.digest).await?.is_none()
                        };
                        if vanished {
                            // The cut fell between the removal and the
                            // confirmation: the real state says the marked
                            // version is no longer there, and that
                            // confirms it (post-commit verdict).
                            self.confirm(&lease, &run, candidate, trace.marker.as_deref())
                                .await?;
                            deleted += 1;
                            bytes_freed += trace.length;
                            actions.insert(key, PassAction::Deleted);
                            continue;
                        }
                        // Still present: the destination is retried,
                        // idempotent under the version guard.
                    }
                }
            }

            // Post-census revalidation: the fresh authority. A
            // concurrent publisher that named the blob takes it out here
            // without touching a byte (AC1: live references are never
            // deleted).
            let live = self
                .ledger()
                .live_reference_count(&candidate.digest)
                .await
                .map_err(|error| {
                    GcError::Unknown(format!("the references could not be read: {error}"))
                })?;
            if live > 0 {
                republished += 1;
                actions.insert(key, PassAction::Republished);
                continue;
            }

            // AC2/fail-closed: without guard capability in the store,
            // this pass does NOT delete. It stays `Unknown`.
            if mode == StoreMode::Refuse {
                unknown += 1;
                actions.insert(key, PassAction::Skipped);
                continue;
            }

            // The version mark is what makes the removal safe:
            // without an observable version there is no guard, and it is not touched.
            let Some(marker) = self.version_of(&candidate.digest).await? else {
                actions.insert(key, PassAction::Gone);
                continue;
            };

            let intended = GcDeletion {
                digest: candidate.digest.clone(),
                length: candidate.length,
                run_id: run.id.clone(),
                actor: self.config.actor().to_string(),
                account: self.config.scope.clone(),
                state: DeletionState::Intended,
                at: Self::now(),
                marker: Some(marker.marker.clone()),
            };
            self.ledger()
                .record_deletion_fenced(&self.config.scope, &lease, &intended)
                .await
                .map_err(Self::busy_or_ledger)?;

            let action = self.retire(&lease, &run, candidate, &marker, mode).await?;
            match action {
                PassAction::Deleted => {
                    deleted += 1;
                    bytes_freed += candidate.length;
                }
                PassAction::Quarantined => quarantined += 1,
                PassAction::NotApplied => not_applied += 1,
                PassAction::Republished => republished += 1,
                PassAction::Skipped => unknown += 1,
                PassAction::Gone => {}
            }
            actions.insert(key, action);
        }

        // 3. Fenced close under the current generation.
        lease = self.renew(&lease).await?;
        self.ledger()
            .complete_scoped_run(
                &self.config.scope,
                &lease,
                &run.id,
                deleted,
                bytes_freed,
                quarantined,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)?;
        self.service.invalidate_plan_cache();
        // The pass is over: hand the claim back so the next process does
        // not wait for the lease to run out. A failure here is harmless,
        // the lease expires by itself.
        if let Err(error) = self
            .ledger()
            .release_gc_lease(&self.config.scope, &lease)
            .await
        {
            tracing::warn!(%error, "the claim of the finished pass was not released");
        }

        Ok(PassReport {
            run_id: run.id,
            generation: lease.generation,
            resumed,
            marked: run.candidates.len() as u64,
            deleted,
            bytes_freed,
            quarantined,
            republished,
            not_applied,
            unknown,
            census_pages,
            actions,
        })
    }

    /// Renews its own claim between marks. A previous generation
    /// token cannot renew: PreconditionFailed -> Busy -> the
    /// pass stops deleting.
    async fn renew(&self, lease: &GcLease) -> Result<GcLease, GcError> {
        self.ledger()
            .renew_gc_lease(
                &self.config.scope,
                lease,
                &self.config.owner,
                self.config.lease_for,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)
    }

    /// The blob's current version mark, if it is still present.
    async fn version_of(&self, digest: &ContentDigest) -> Result<Option<VersionMarker>, GcError> {
        self.service
            .store()
            .conditional()
            .ok_or(GcError::Unsupported)?
            .head_marker(digest)
            .await
            .map_err(|error| GcError::Store(format!("{error:?}")))
    }

    /// The destination of an already marked candidate, according to the
    /// store's mode. The trail's confirmation only goes after the
    /// action has been APPLIED (post-effect verdict).
    async fn retire(
        &self,
        lease: &GcLease,
        run: &GcRun,
        candidate: &RunCandidate,
        marker: &VersionMarker,
        mode: StoreMode,
    ) -> Result<PassAction, GcError> {
        let blob: &dyn ConditionalBlob = self
            .service
            .store()
            .conditional()
            .ok_or(GcError::Unsupported)?;
        match mode {
            StoreMode::Conditional => match blob
                .delete_if_untouched(&candidate.digest, marker)
                .await
                .map_err(|error| GcError::Store(format!("{error:?}")))?
            {
                ConditionalOutcome::Applied { .. } => {
                    self.confirm(lease, run, candidate, Some(&marker.marker))
                        .await?;
                    Ok(PassAction::Deleted)
                }
                ConditionalOutcome::Gone => Ok(PassAction::Gone),
                // AC2: the marked version is no longer the current one. The
                // new one is not touched and its key is barred from
                // reuse until reconciled: the record keeps
                // the attempted mark and `NotApplied` is reported, it is not
                // re-deleted "to see if it works this time".
                ConditionalOutcome::NotApplied => Ok(PassAction::NotApplied),
            },
            StoreMode::Quarantine => {
                // Versioned quarantine: NEW name, never a
                // rewrite of the original key.
                let key = blob.quarantine_key_of(&candidate.digest, &run.id);
                match blob
                    .quarantine_if_untouched(&candidate.digest, marker, &key)
                    .await
                    .map_err(|error| GcError::Store(format!("{error:?}")))?
                {
                    ConditionalOutcome::Applied { length } => {
                        self.ledger()
                            .record_quarantine(
                                &self.config.scope,
                                lease,
                                &Quarantined {
                                    digest: candidate.digest.clone(),
                                    length,
                                    run_id: run.id.clone(),
                                    quarantine_key: key,
                                    marker: marker.marker.clone(),
                                    at: Self::now(),
                                },
                            )
                            .await
                            .map_err(Self::busy_or_ledger)?;
                        self.confirm(lease, run, candidate, Some(&marker.marker))
                            .await?;
                        Ok(PassAction::Quarantined)
                    }
                    ConditionalOutcome::Gone => Ok(PassAction::Gone),
                    ConditionalOutcome::NotApplied => Ok(PassAction::NotApplied),
                }
            }
            StoreMode::Refuse => Ok(PassAction::Skipped),
        }
    }

    /// Confirms the trail under the current claim.
    async fn confirm(
        &self,
        lease: &GcLease,
        run: &GcRun,
        candidate: &RunCandidate,
        marker: Option<&str>,
    ) -> Result<(), GcError> {
        let confirmed = GcDeletion {
            digest: candidate.digest.clone(),
            length: candidate.length,
            run_id: run.id.clone(),
            actor: self.config.actor().to_string(),
            account: self.config.scope.clone(),
            state: DeletionState::Confirmed,
            at: Self::now(),
            marker: marker.map(str::to_string),
        };
        self.ledger()
            .record_deletion_fenced(&self.config.scope, lease, &confirmed)
            .await
            .map_err(Self::busy_or_ledger)?;
        Ok(())
    }
}
