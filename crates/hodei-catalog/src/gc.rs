//! The content census and the collection ledger.
//!
//! The catalog is the only source of **roots**: what says a blob
//! is alive is a real row (an asset, a reference, a version), not
//! a counter. This module defines what the collector reads and writes, and
//! decides nothing: whoever decides is the collector's plan, with this
//! data.
//!
//! # What the ledger keeps
//!
//! - The **holds**: manual retentions (legal hold) over a digest, a
//!   repository or an account. They can expire.
//! - The **runs**: the durable mark of a collection. A run
//!   marked and not completed is what resumes after a restart.
//! - The **deletions**: the trace of each blob that was deleted, with who
//!   requested it and in which run.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::CatalogError;

/// Which catalog row a root comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKind {
    /// The asset row: the path points to this content.
    Asset,
    /// The reference to the blob the path now serves.
    Reference,
    /// The published version: it keeps the digest even if the asset moves.
    Version,
    /// The `subject` of a manifest: a referrer hangs from this digest.
    Subject,
}

impl RootKind {
    /// The stable name, as it appears in reports.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Reference => "reference",
            Self::Version => "version",
            Self::Subject => "subject",
        }
    }
}

/// A root: a real catalog row that keeps a content alive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRoot {
    /// Which row it comes from.
    pub kind: RootKind,
    /// The owning account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The path or the version that references it.
    pub subject: String,
    /// The content it keeps alive.
    pub digest: ContentDigest,
}

/// What a hold reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoldScope {
    /// A specific content.
    Digest {
        /// The retained content.
        digest: ContentDigest,
    },
    /// Everything a repository references.
    Repository {
        /// The account.
        account: String,
        /// The repository.
        repository: String,
    },
    /// Everything an account references.
    Account {
        /// The account.
        account: String,
    },
}

/// A hold: what it reaches is not reclaimable while it lasts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentHold {
    /// The identifier of the hold.
    pub id: String,
    /// What it reaches.
    pub scope: HoldScope,
    /// Why it was placed.
    pub reason: String,
    /// Who placed it.
    pub actor: String,
    /// When it was placed.
    pub created_at: DateTime<Utc>,
    /// When it expires. Without a date, it lasts until someone releases it.
    pub expires_at: Option<DateTime<Utc>>,
}

impl ContentHold {
    /// Whether the hold is still in force at `now`.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }
}

/// Where a collection run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// The candidates are marked and the sweep has not finished.
    Marked,
    /// The sweep finished.
    Swept,
}

impl RunState {
    /// The stable name, as it goes in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Marked => "marked",
            Self::Swept => "swept",
        }
    }
}

/// A candidate marked in a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCandidate {
    /// The marked content.
    pub digest: ContentDigest,
    /// The bytes it occupies.
    pub length: u64,
}

/// A collection run: the durable mark that survives a cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcRun {
    /// The identifier of the run.
    pub id: String,
    /// Where it stands.
    pub state: RunState,
    /// The grace period it was marked with, in seconds.
    pub grace_secs: u64,
    /// Who launched it.
    pub actor: String,
    /// The marked candidates.
    pub candidates: Vec<RunCandidate>,
    /// When it was marked.
    pub marked_at: DateTime<Utc>,
    /// How many blobs were deleted.
    pub deleted: u64,
    /// How many bytes were freed.
    pub bytes_freed: u64,
    /// When the sweep finished.
    pub swept_at: Option<DateTime<Utc>>,
    /// The authority scope under which this run was marked (coordinated
    /// mode, HSP-192). Empty in legacy mode.
    #[serde(default)]
    pub scope: Option<String>,
    /// The generation of the claim that opened the run. A zombie of an
    /// earlier generation can read the mark but does not sweep under it.
    #[serde(default)]
    pub generation: Option<u64>,
}

/// Whether the deletion already happened or is only intended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionState {
    /// The intent was recorded and it is not yet known whether the deletion happened.
    Intended,
    /// The store confirmed the deletion.
    Confirmed,
}

impl DeletionState {
    /// The stable name, as it goes in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Intended => "intended",
            Self::Confirmed => "confirmed",
        }
    }
}

/// The trace of a blob's deletion.
///
/// It is written **before** deleting (`Intended`) and confirmed afterwards: a
/// cut between the two leaves the intent recorded, and the restart
/// confirms it upon seeing that the blob is no longer there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcDeletion {
    /// The deleted content.
    pub digest: ContentDigest,
    /// The bytes it occupied.
    pub length: u64,
    /// The run that deleted it.
    pub run_id: String,
    /// Who requested it.
    pub actor: String,
    /// The verified account of whoever requested it.
    pub account: String,
    /// Whether the deletion was confirmed.
    pub state: DeletionState,
    /// When it was recorded.
    pub at: DateTime<Utc>,
    /// The version mark of the object store that the run observed when
    /// intending the deletion (coordinated mode, HSP-192). Empty in
    /// legacy mode.
    #[serde(default)]
    pub marker: Option<String>,
}

/// What a run left in quarantine (HSP-192, AC2).
///
/// Quarantine is versioned: the marked bytes are copied to a NEW
/// name, the original key is never rewritten. While the quarantine
/// is not reconciled, the key stays **reserved**: neither the GC itself
/// touches it again nor does the run consider it reclaimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quarantined {
    /// The marked content.
    pub digest: ContentDigest,
    /// The bytes the quarantined version occupied.
    pub length: u64,
    /// The run that quarantined it.
    pub run_id: String,
    /// The versioned quarantine key where the object is.
    pub quarantine_key: String,
    /// The mark of the quarantined version (ETag, mtime... whatever the
    /// store uses to say "this exact version").
    pub marker: String,
    /// When it was quarantined.
    pub at: DateTime<Utc>,
}

/// A page of roots read from the catalog (HSP-192, AC3).
///
/// The census does not materialize the whole graph: it is walked in pages
/// bounded by `limit`, with the cursor of the previous page. A
/// page full to `limit` signals that there are more; a shorter
/// page marks the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPage {
    /// The roots of this page.
    pub roots: Vec<ContentRoot>,
    /// Whether the catalog said that more roots fit after this page.
    pub has_more: bool,
}

/// The fenced claim of a run (HSP-192, AC1).
///
/// The claim is DURABLE --it lives in the graph's authority, not in the
/// worker's memory-- and expires at `lease_until`. The token is the
/// fencing bound: EVERY action of the run (mark, delete, quarantine,
/// confirm) must carry the token of the current generation, and an
/// old token is rejected even if the old worker's process is still alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcLease {
    /// The monotonic generation of the claim: it rises every time someone
    /// takes control, including a take-over of an expired lease.
    pub generation: u64,
    /// The opaque identifier of this claim: what the worker presents
    /// in each operation. A resurrected worker with the previous generation
    /// cannot present the new one's token.
    pub token: String,
    /// The instant at which the claim stops being valid.
    pub lease_until: DateTime<Utc>,
}

/// The collector's ledger over the catalog.
///
/// It does not authorize: it is store administration, like `initialize`. The
/// gate is the hub's administration API, which decides by route.
///
/// # Coordination between replicas (HSP-192)
///
/// The new methods have a default implementation that fails
/// closed: a ledger without a fenced claim **cannot** take part in a
/// coordinated run, and the error says so in the type. The legacy mode
/// (`sweep`) does not use them and keeps working the same.
#[async_trait]
pub trait ContentLedger: Send + Sync {
    /// All the roots of the catalog.
    ///
    /// If not all of them can be read, it is an error: a partial census is not a
    /// census, and whoever collects with it cannot tell the orphaned from
    /// what was not seen.
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError>;

    /// The registered holds, in force or not.
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError>;

    /// Registers a hold. Repeating the same `id` replaces it.
    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError>;

    /// Releases a hold. Returns whether it existed.
    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError>;

    /// The marked and uncompleted run, if there is one.
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError>;

    /// The last completed run, if there is one.
    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError>;

    /// Writes the mark of a run.
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError>;

    /// Closes a run with its totals.
    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError>;

    /// Writes or confirms the trace of a blob's deletion.
    ///
    /// On confirming, it also writes the audit event. Confirming
    /// twice does not repeat the event.
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError>;

    /// The trace of a content's deletion, if there is one.
    async fn deletion_of(&self, digest: &ContentDigest)
    -> Result<Option<GcDeletion>, CatalogError>;

    // ------------------------------------------------------------------
    // Coordinated, fenced pass (HSP-192). Defaults fail closed: a ledger
    // that cannot fence cannot participate; the legacy mode never calls
    // these and is untouched.
    // ------------------------------------------------------------------

    /// Whether this ledger fences a coordinated pass at all.
    ///
    /// The API surface reads this to refuse a coordinated request with a
    /// typed error instead of pretending. The default says no.
    fn supports_fenced_pass(&self) -> bool {
        false
    }

    /// Take or renew the durable lease of a scope (claim fenced).
    ///
    /// - If no lease exists, or the stored one is expired at `now`, the
    ///   implementation MUST create a NEW generation and token — this is
    ///   how a zombie holder of the previous generation gets fenced.
    /// - If a live lease exists held by `owner`, renew it and hand back
    ///   the SAME token (idempotent for its own holder).
    /// - If a live lease exists held by another owner, report busy.
    ///
    /// Success is decided by reading the claim back after the commit,
    /// never by the transaction merely having committed.
    async fn claim_gc_pass(
        &self,
        scope: &str,
        owner: &str,
        lease: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let _ = (scope, owner, lease, now);
        Err(CatalogError::unavailable(
            "claim_gc_pass: this ledger cannot fence a coordinated pass",
        ))
    }

    /// The live lease of a scope, if one exists and has not expired at
    /// `now`. A read, never a claim.
    async fn current_gc_lease(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        let _ = (scope, now);
        Err(CatalogError::unavailable(
            "current_gc_lease: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Mark a run of candidates inside ONE fenced, guarded transaction.
    ///
    /// The implementation MUST, in the same transaction:
    /// - verify `lease` is still the live claim of `scope` (a stale
    ///   token aborts with `PreconditionFailed`);
    /// - verify that none of the candidate digests is referenced by the
    ///   live catalog (the reference guard): a digest that gained a root
    ///   after the census is NOT marked, and the whole mark aborts
    ///   rather than racing the publisher;
    /// - write the durable mark (the run) so a crash between mark and
    ///   sweep resumes from the ledger, not from memory.
    ///
    /// Returns the candidates as actually marked (post-commit read of
    /// the mark, per the HSP-205 verdict-after-commit rule).
    async fn mark_run_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
        candidate_digests: &[ContentDigest],
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        let _ = (scope, lease, run, candidate_digests);
        Err(CatalogError::unavailable(
            "mark_run_fenced: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Re-confirm the durable mark of `run_id` for the CURRENT live
    /// lease, for a resumed or long pass.
    ///
    /// Fencing is by the presented token, not by the stamp: a take-over
    /// of an expired claim MAY resume the pending mark of the dead
    /// generation (that is the point of the durable mark), but only
    /// while it presents the live token. A zombie whose lease was taken
    /// over gets `PreconditionFailed` here and cannot sweep.
    async fn mark_of_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        let _ = (scope, lease, run_id);
        Err(CatalogError::unavailable(
            "mark_of_run: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Write or confirm the fenced deletion trace.
    ///
    /// Same contract as [`Self::record_deletion`], but the write carries
    /// the lease and MUST abort if the generation no longer matches: a
    /// zombie worker cannot even record that it deleted.
    async fn record_deletion_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        let _ = (scope, lease, deletion);
        Err(CatalogError::unavailable(
            "record_deletion_fenced: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Record that a marked version was quarantined instead of deleted
    /// (AC2 path), reserving its catalog key until reconciliation.
    async fn record_quarantine(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        let _ = (scope, lease, quarantine);
        Err(CatalogError::unavailable(
            "record_quarantine: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Read one bounded page of catalog roots (AC3: the plan census must
    /// never materialize the whole graph).
    ///
    /// `after` is the cursor from the previous page (an opaque digest
    /// key); `None` starts at the beginning. Pages are disjoint and
    /// ordered by digest so consecutive passes converge.
    async fn root_page(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        let _ = (scope, after, limit);
        Err(CatalogError::unavailable(
            "root_page: this ledger cannot page the census",
        ))
    }

    /// The quarantines recorded under `run_id`, if any (for resume and
    /// for the reconciliation surface).
    async fn quarantines_of_run(&self, run_id: &str) -> Result<Vec<Quarantined>, CatalogError> {
        let _ = run_id;
        Ok(Vec::new())
    }

    /// The pending (marked, not completed) run of a coordinated scope.
    ///
    /// A worker that restarts resumes through this: the mark is durable
    /// in the authority, not in the process. A run marked by another
    /// scope's generation is not this scope's pending run.
    async fn pending_scoped_run(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        let _ = (scope, now);
        Err(CatalogError::unavailable(
            "pending_scoped_run: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Renew the lease of the CURRENT generation for its own owner.
    ///
    /// The worker calls this between marks to extend the claim while it
    /// sweeps. A token that is not the current generation's cannot
    /// renew: the answer is a typed precondition failure, and the pass
    /// stops deleting.
    async fn renew_gc_lease(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let _ = (scope, lease, owner, lease_for, now);
        Err(CatalogError::unavailable(
            "renew_gc_lease: this ledger cannot fence a coordinated pass",
        ))
    }

    /// The post-commit reference probe (HSP-192: verify AFTER commit).
    ///
    /// Counts the catalog references to `digest` read FRESH from the
    /// authority, after the mark committed. A nonzero count means a
    /// publisher named the blob while the pass was marking: the
    /// candidate is dropped from the sweep, never deleted. This is the
    /// HSP-205 rule — the verdict comes from the state after the
    /// commit, not from the transaction merely having committed.
    async fn live_reference_count(&self, digest: &ContentDigest) -> Result<u64, CatalogError> {
        // The legacy ledgers can only see the direct roots from here.
        // Implementations that hold the graph MUST override and see the
        // whole picture, including manifest edges.
        let roots = self.content_roots().await?;
        Ok(roots.iter().filter(|root| &root.digest == digest).count() as u64)
    }

    /// Close a coordinated run as swept for its generation.
    ///
    /// The counters travel with the close because the report is what a
    /// resumed pass replays; the shape is the contract, not an
    /// oversight.
    #[allow(clippy::too_many_arguments)]
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
        let _ = (scope, lease, run_id, deleted, bytes_freed, quarantined, at);
        Err(CatalogError::unavailable(
            "complete_scoped_run: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Give the claim of `scope` back once its pass is over.
    ///
    /// A pass that finished has nothing left to protect: without this a
    /// replacement process (a restart, another replica) would find the
    /// scope held by the finished owner until the lease ran out. Fenced
    /// like every other operation: only the live token releases. The
    /// default is a no-op, the lease then simply expires.
    async fn release_gc_lease(&self, scope: &str, lease: &GcLease) -> Result<(), CatalogError> {
        let _ = (scope, lease);
        Ok(())
    }

    /// The accounts whose roots make up the whole census of the store.
    ///
    /// The content store is shared by every account, so a platform-wide
    /// collection pages the roots of each of them (`root_page` per
    /// account) instead of asking for one global materialised census.
    /// The answer is bounded by the number of accounts, never by the
    /// number of roots.
    async fn census_accounts(&self) -> Result<Vec<String>, CatalogError> {
        Err(CatalogError::unavailable(
            "census_accounts: this ledger cannot page the census",
        ))
    }
}
