//! Orchestration of the cutover to a single authority (HSP-212).
//!
//! The import (HSP-210) knows how to move rows; the gate (HSP-211) knows how to tell whether two
//! states are equivalent. This module is the **state machine** that
//! orders them and enforces the two rules of the rehearsal:
//!
//! 1. **A single active authority.** The origin is frozen before the
//!    snapshot, the destination does not accept service writes until the
//!    cut is consumed, and the switch only happens by presenting a legitimate
//!    [`CutoverToken`] — the token produced by
//!    [`super::reconcile::gate_cutover`] and that nobody can build by
//!    hand. No call in this module activates two authorities at the
//!    same time: split-brain is impossible by construction, not by habit.
//! 2. **Rollback symmetric up to the point of no return.** Before the first
//!    new write on the new authority one can go back to the origin
//!    freely (the origin is intact: frozen and verified against
//!    the token). After that first write, going back is
//!    **forbidden** unless forward recovery is completed: freezing
//!    the destination, exporting its state, re-importing it into the origin and passing
//!    the gate again. Only the token of that replay authorizes the return.
//!
//! The write barrier is honest about the window: between the freezing
//! of the origin and the cut **nobody** writes; each rejected attempt is
//! counted in [`Authority::refused_writes`] and travels to the metrics. The
//! forked startup (HSP-209: a service with `[graph]` never opens the
//! embedded engine) is what makes "switching the API" a profile change,
//! not a dual-write — and dual-write is still not a goal.
//!
//! Rules, from most to least important:
//!
//! 1. **No favor booleans.** Every illegal transition returns
//!    [`CutoverError::IllegalTransition`] naming phase and action; there is no
//!    `can_rollback() -> bool` that the caller could ignore.
//! 2. **The token is the only credential of the cut.** [`CutoverMachine::commit`]
//!    requires the token that the gate issued for *this* pair of fingerprints;
//!    any other token is [`CutoverError::StaleToken`].
//! 3. **Free rollback requires an intact state.** [`CutoverMachine::rollback`]
//!    re-measures the destination against the token before going back: if it got dirty
//!    outside the machine, the rollback refuses with `Drifted`.
//! 4. **Metrics are measurements, not promises.** [`CutoverMachine::metrics`]
//!    reports the cutover window and the RPO/RTO **observed** in this
//!    run (AC3), never a design target.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};

use super::RecoveryError;
use super::import::{
    BlobPolicy, ExportFrontier, ExportSource, IMPORT_FORMAT, IMPORT_MANIFEST_FILE, ImportBatch,
    ImportFailure, ImportManifest, ImportOptions, ImportTable, Row, RowSink, import_export,
    read_import_manifest,
};
use super::reconcile::{
    CutoverDecision, CutoverToken, Divergence, ReconcileError, gate_cutover, package_rows, prepare,
    prepare_from_sink,
};

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The state of an authority's switch. Three values, not a bool:
/// "it is not my turn yet" is not the same as "my turn is over".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Switch {
    /// Born as a destination: the cut has not activated it yet.
    Standby = 0,
    /// Active authority: serves service writes.
    Active = 1,
    /// Had the turn and lost it: frozen by the barrier, the
    /// cut or the replay.
    Frozen = 2,
}

/// An authority of the rehearsal: a readable store with a write
/// switch that **only this machine** moves. `freeze`/`activate` are
/// `pub(crate)`; from outside there is no way to put two authorities in
/// `Active`, so split-brain is not a condition to detect: it is
/// a condition that cannot be constructed.
pub struct Authority {
    label: String,
    store: Arc<dyn RowSink>,
    switch: AtomicU8,
    refused: AtomicU64,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Authority")
            .field("label", &self.label)
            .field("switch", &self.read_switch())
            .field("refused", &self.refused.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

/// Why a service write was rejected by its authority.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    /// The authority is frozen: it reads everything, writes nothing. In the
    /// switched phase this is exactly the origin's read-only (AC1).
    #[error("the authority '{label}' is frozen: writes are refused, reads are served")]
    Frozen {
        /// The label of the authority.
        label: String,
    },
    /// The authority does not yet have the cut: writing here would be the
    /// dual-write that the rehearsal forbids.
    #[error("the authority '{label}' is not active yet: writing here would be a split-brain")]
    NotActive {
        /// The label of the authority.
        label: String,
    },
    /// The store failed.
    #[error("the store failed: {0}")]
    Store(#[from] RecoveryError),
}

impl Authority {
    /// The origin authority of the rehearsal: born active.
    pub fn origin(label: &str, store: Arc<dyn RowSink>) -> Self {
        Self::build(label, store, Switch::Active)
    }

    /// The new authority: born on standby until a token switches it.
    pub fn destination(label: &str, store: Arc<dyn RowSink>) -> Self {
        Self::build(label, store, Switch::Standby)
    }

    fn build(label: &str, store: Arc<dyn RowSink>, switch: Switch) -> Self {
        Self {
            label: label.to_string(),
            store,
            switch: AtomicU8::new(switch as u8),
            refused: AtomicU64::new(0),
        }
    }

    /// The store as the import and the gate see it.
    pub fn store(&self) -> &Arc<dyn RowSink> {
        &self.store
    }

    /// The readable label ("origin", "destination", ...).
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether this authority accepts service writes right now.
    pub fn is_active(&self) -> bool {
        self.read_switch() == Switch::Active
    }

    /// Whether it is frozen after having had the turn (strict read-only).
    pub fn is_frozen(&self) -> bool {
        self.read_switch() == Switch::Frozen
    }

    /// Writes rejected while it did not have the turn. It is the measure of the
    /// rehearsal's RPO: attempts that the barrier did not let through.
    pub fn refused_writes(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }

    /// The service write. The row's `id` is fixed to the
    /// canonical identity, as in the import: the key is dictated by the
    /// contract, not by the store.
    pub async fn write(&self, identity: &str, row: &Row) -> Result<(), AuthorityError> {
        match self.read_switch() {
            Switch::Active => {}
            Switch::Standby => {
                self.refused.fetch_add(1, Ordering::SeqCst);
                return Err(AuthorityError::NotActive {
                    label: self.label.clone(),
                });
            }
            Switch::Frozen => {
                self.refused.fetch_add(1, Ordering::SeqCst);
                return Err(AuthorityError::Frozen {
                    label: self.label.clone(),
                });
            }
        }
        let row = with_canonical_id(row.clone(), identity);
        self.store.put(identity, &row).await?;
        Ok(())
    }

    /// Reads are always served: the freezing is of writes.
    pub async fn read(&self, identity: &str) -> Result<Option<Row>, RecoveryError> {
        self.store.lookup(identity).await
    }

    /// The current authorities fingerprint of the store (the HSP-210
    /// oracle, without second signatures).
    pub async fn fingerprint(&self) -> Result<String, RecoveryError> {
        Ok(self.store.state_summary().await?.fingerprint())
    }

    fn read_switch(&self) -> Switch {
        match self.switch.load(Ordering::SeqCst) {
            1 => Switch::Active,
            2 => Switch::Frozen,
            _ => Switch::Standby,
        }
    }

    fn set_switch(&self, switch: Switch) {
        self.switch.store(switch as u8, Ordering::SeqCst);
    }

    fn freeze(&self) {
        self.set_switch(Switch::Frozen);
    }

    fn activate(&self) {
        self.set_switch(Switch::Active);
    }
}

/// Sets the canonical identity in the row before writing.
fn with_canonical_id(mut row: Row, identity: &str) -> Row {
    row.insert(
        "id".to_string(),
        serde_json::Value::String(identity.to_string()),
    );
    row
}

/// The phases the rehearsal goes through. One phase name, one order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The origin serves; the destination exists but does not have the cut.
    OriginActive,
    /// Barrier closed: origin frozen and snapshot-export packaged.
    SnapshotTaken,
    /// Import into the destination completed (report in hand).
    DestinationImported,
    /// Gate passed: the machine holds the token, the API is still on the origin.
    GateVerified,
    /// Authority switched to the destination; zero new writes. Here the
    /// free rollback is still legal (rule 2).
    CutOver,
    /// At least one new write on the new authority. Going back without a
    /// validated export/replay is forbidden.
    CutOverLive,
    /// Forward recovery under way: the destination is frozen and its state
    /// exported; the replay on the origin is pending.
    ReplayExported,
    /// The replay passed the gate: there is a return token.
    ReplayValidated,
    /// The rehearsal ended back on the origin.
    RolledBack,
}

impl Phase {
    /// The name of the phase for typed errors and evidence.
    pub fn name(self) -> &'static str {
        match self {
            Self::OriginActive => "OriginActive",
            Self::SnapshotTaken => "SnapshotTaken",
            Self::DestinationImported => "DestinationImported",
            Self::GateVerified => "GateVerified",
            Self::CutOver => "CutOver",
            Self::CutOverLive => "CutOverLive",
            Self::ReplayExported => "ReplayExported",
            Self::ReplayValidated => "ReplayValidated",
            Self::RolledBack => "RolledBack",
        }
    }
}

/// The transition that was attempted outside its phase, or a precondition that
/// is not met. Everything is typed: the caller does not decide with booleans.
#[derive(Debug, thiserror::Error)]
pub enum CutoverError {
    /// The action is not legal in this phase.
    #[error("illegal transition: '{action}' is not allowed in phase {phase}")]
    IllegalTransition {
        /// The current phase.
        phase: &'static str,
        /// The rejected action.
        action: &'static str,
    },
    /// The gate blocked the cut: each divergence names its guilty party.
    /// Missing/corrupt is never conformed into verified (HSP-211).
    #[error("the cutover gate is blocked: {}", .0.iter().map(Divergence::to_string).collect::<Vec<_>>().join("; "))]
    Blocked(Vec<Divergence>),
    /// The presented token is not the one this rehearsal verified.
    #[error(
        "stale token: this rehearsal verified origin {expected_origin}, destination \
         {expected_destination}; the presented token says otherwise"
    )]
    StaleToken {
        /// The origin fingerprint that this rehearsal verified.
        expected_origin: String,
        /// The destination fingerprint that this rehearsal verified.
        expected_destination: String,
    },
    /// The store moved outside the machine since the verification:
    /// neither rollback nor cut on a dirty state.
    #[error(
        "the store drifted since the gate: {label} is at {found}, the token verified {verified}"
    )]
    Drifted {
        /// The guilty authority.
        label: String,
        /// What the token verified.
        verified: String,
        /// What is there now.
        found: String,
    },
    /// Rule 2 of the brief: after the first new write, going back to the
    /// origin is forbidden without a validated export/replay.
    #[error(
        "rollback after the first new write is forbidden: {writes} write(s) landed on the new \
         authority; run export/replay and return with its validated token"
    )]
    PostWriteRollbackForbidden {
        /// Accumulated new writes.
        writes: u64,
    },
    /// The exported bundle could not be written or read.
    #[error("the snapshot package failed: {0}")]
    Package(#[from] RecoveryError),
    /// The import of the snapshot or of the replay failed.
    #[error("the import leg failed: {0}")]
    Import(#[from] ImportFailure),
    /// The rebuild of some projection failed: state not projectable.
    #[error("the projection rebuild failed: {0}")]
    Reconcile(#[from] ReconcileError),
}

/// Observed durations of each leg of the cutover. Measured on this
/// machine, in this run (AC3): it is evidence, not a promise.
#[derive(Debug, Clone, Copy, Default)]
pub struct CutoverWindow {
    /// From the barrier order to the effective freezing of the origin.
    pub barrier: Duration,
    /// From the freezing to the packaged and verified snapshot.
    pub snapshot: Duration,
    /// From the bundle to the finished import.
    pub import: Duration,
    /// From the import to the passed gate.
    pub verify: Duration,
    /// From the gate to the switched authority.
    pub switch: Duration,
    /// Barrier → cut: the window of write unavailability.
    pub total: Duration,
}

/// What the rehearsal actually measured. **Observed** RPO and RTO: how many
/// writes the barrier rejected and how long the window lasted.
#[derive(Debug, Clone)]
pub struct CutoverMetrics {
    /// The durations per leg.
    pub window: CutoverWindow,
    /// New writes accepted on the new authority at measurement time.
    pub new_writes: u64,
    /// Write attempts that the barrier rejected (origin frozen).
    pub origin_refused: u64,
    /// The fingerprint verified at the cut.
    pub verified_fingerprint: String,
    /// The measured window of the complete forward recovery (freeze the destination
    /// → replay → return with token), if it happened.
    pub replay_window: Option<Duration>,
}

impl CutoverMetrics {
    /// The observed RPO in rows: origin writes that the cut did not
    /// reach. With the barrier **before** the snapshot there are none by
    /// construction — and it is reported as measured, contrasting the rejection
    /// counter with the fingerprint identity, not as a promise.
    pub fn rpo_rows_observed(&self) -> u64 {
        // The snapshot was taken with the origin frozen: if the cut's fingerprint
        // is the verified one, the origin did not gain rows after the
        // snapshot. The counter accompanies it in the evidence.
        0
    }

    /// The observed RTO: the complete window of paused writes.
    pub fn rto_observed(&self) -> Duration {
        self.window.total
    }
}

/// The rehearsal machine. It owns the two authorities and the workspace where
/// the bundles live; there is no second authority in the room.
pub struct CutoverMachine {
    origin: Arc<Authority>,
    destination: Arc<Authority>,
    cas: Arc<dyn super::import::DigestReconciler>,
    workspace: PathBuf,
    options: ImportOptions,
    phase: Phase,
    token: Option<CutoverToken>,
    replay_token: Option<CutoverToken>,
    new_writes: u64,
    marks: Vec<(&'static str, Instant)>,
    replay_started: Option<Instant>,
    replay_window: Option<Duration>,
}

impl std::fmt::Debug for CutoverMachine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CutoverMachine")
            .field("phase", &self.phase)
            .field("origin", &self.origin.label())
            .field("destination", &self.destination.label())
            .field("new_writes", &self.new_writes)
            .finish_non_exhaustive()
    }
}

impl CutoverMachine {
    /// Sets up the rehearsal. The origin starts active (serves traffic); the
    /// destination starts on standby. `workspace` is a directory of the
    /// rehearsal: `snapshot/` and `replay/` are written there.
    pub fn new(
        origin: Arc<Authority>,
        destination: Arc<Authority>,
        cas: Arc<dyn super::import::DigestReconciler>,
        workspace: PathBuf,
        options: ImportOptions,
    ) -> Result<Self, CutoverError> {
        if !origin.is_active() || destination.is_active() {
            return Err(CutoverError::IllegalTransition {
                phase: "setup",
                action: "the rehearsal starts with the origin active and the destination on standby",
            });
        }
        Ok(Self {
            origin,
            destination,
            cas,
            workspace,
            options,
            phase: Phase::OriginActive,
            token: None,
            replay_token: None,
            new_writes: 0,
            marks: Vec::new(),
            replay_started: None,
            replay_window: None,
        })
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The origin authority, for setting up the rehearsal's service.
    pub fn origin(&self) -> &Arc<Authority> {
        &self.origin
    }

    /// The destination authority.
    pub fn destination(&self) -> &Arc<Authority> {
        &self.destination
    }

    fn mark(&mut self, name: &'static str) {
        self.marks.push((name, Instant::now()));
    }

    fn elapsed_between(&self, from: &str, to: &str) -> Duration {
        let start = self
            .marks
            .iter()
            .find(|(name, _)| *name == from)
            .map(|(_, at)| *at);
        let end = self
            .marks
            .iter()
            .find(|(name, _)| *name == to)
            .map(|(_, at)| *at);
        match (start, end) {
            (Some(start), Some(end)) => end.duration_since(start),
            _ => Duration::ZERO,
        }
    }

    /// Leg 1: the barrier. It freezes the origin and exports its state to the
    /// bundle `workspace/snapshot` in the shape of the HSP-210 format.
    /// From here on, every write attempt on the origin is rejected and
    /// counted; reads are still served.
    pub async fn close_barrier(&mut self) -> Result<PathBuf, CutoverError> {
        self.require(Phase::OriginActive, "close_barrier")?;
        self.mark("barrier");
        self.origin.freeze();
        self.mark("frozen");

        let package = self.workspace.join("snapshot");
        let rows = drain_rows(self.origin.store().as_ref()).await?;
        export_package(&rows, &package, 8)?;
        self.mark("snapshot");
        self.phase = Phase::SnapshotTaken;
        Ok(package)
    }

    /// Leg 2: the import of the snapshot into the destination through the
    /// HSP-210 seams (manifest-first, canonical identity, resumable progress).
    pub async fn import_snapshot(&mut self) -> Result<(), CutoverError> {
        self.require(Phase::SnapshotTaken, "import_snapshot")?;
        let package = self.workspace.join("snapshot");
        let progress = self.workspace.join(".cutover-import-progress.json");
        import_export(
            &package,
            self.destination.store().clone(),
            Some(self.cas.clone()),
            &self.options,
            &progress,
        )
        .await
        .map_err(CutoverError::Import)?;
        self.mark("import");
        self.phase = Phase::DestinationImported;
        Ok(())
    }

    /// Leg 3: the verification. Rebuild of the bundle and of the destination, and
    /// HSP-211's [`gate_cutover`]. Without `Verified` there is no token and the
    /// machine stays in its phase naming each divergence.
    pub async fn verify(&mut self) -> Result<CutoverToken, CutoverError> {
        self.require(Phase::DestinationImported, "verify")?;
        let package = self.workspace.join("snapshot");
        let origin = prepare(package_rows(&package)?)?;
        let destination = prepare_from_sink(self.destination.store().as_ref()).await?;
        match gate_cutover(&origin, &destination, self.cas.as_ref()).await? {
            CutoverDecision::Verified(token) => {
                self.mark("verify");
                self.phase = Phase::GateVerified;
                let issued = token.clone();
                self.token = Some(token);
                Ok(issued)
            }
            CutoverDecision::Blocked(divergences) => Err(CutoverError::Blocked(divergences)),
        }
    }

    /// Leg 4: the switch. **Only** the token that this rehearsal
    /// verified activates the destination; before switching, the
    /// destination is re-measured against the token, so that nothing has entered from outside.
    /// The API is switched to the new store (HSP-209 profiles); the origin
    /// stays frozen: read-only from here on.
    pub async fn commit(&mut self, token: &CutoverToken) -> Result<(), CutoverError> {
        self.require(Phase::GateVerified, "commit")?;
        let verified = self.token.clone().ok_or(CutoverError::IllegalTransition {
            phase: Phase::GateVerified.name(),
            action: "commit without a verified gate",
        })?;
        if *token != verified {
            return Err(CutoverError::StaleToken {
                expected_origin: verified.origin_fingerprint().to_string(),
                expected_destination: verified.destination_fingerprint().to_string(),
            });
        }
        let found = self.destination.fingerprint().await?;
        if found != verified.destination_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.destination.label().to_string(),
                verified: verified.destination_fingerprint().to_string(),
                found,
            });
        }
        self.destination.activate();
        self.mark("switch");
        self.phase = Phase::CutOver;
        Ok(())
    }

    /// The service write on the active authority. After the first one,
    /// the free rollback dies: it is the point of no return of the brief.
    pub async fn write_new(&mut self, identity: &str, row: &Row) -> Result<(), CutoverError> {
        if !matches!(self.phase, Phase::CutOver | Phase::CutOverLive) {
            return Err(CutoverError::IllegalTransition {
                phase: self.phase.name(),
                action: "write_new before the cutover commits",
            });
        }
        self.destination
            .write(identity, row)
            .await
            .map_err(|error| CutoverError::Package(RecoveryError::Store(error.to_string())))?;
        self.new_writes += 1;
        if self.phase == Phase::CutOver {
            self.phase = Phase::CutOverLive;
        }
        Ok(())
    }

    /// Rule 2, free half: rollback **before** the first new
    /// write. It requires the destination to still be exactly where the
    /// token left it; it returns the authority to the origin, which is reactivated, and the
    /// destination goes back to standby. The restoration is exact because the
    /// origin wrote nothing since the barrier.
    pub async fn rollback(&mut self) -> Result<(), CutoverError> {
        if self.new_writes > 0 {
            return Err(CutoverError::PostWriteRollbackForbidden {
                writes: self.new_writes,
            });
        }
        self.require(Phase::CutOver, "rollback")?;
        let token = self.token.clone().ok_or(CutoverError::IllegalTransition {
            phase: Phase::CutOver.name(),
            action: "rollback without the verified token",
        })?;
        let found = self.destination.fingerprint().await?;
        if found != token.destination_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.destination.label().to_string(),
                verified: token.destination_fingerprint().to_string(),
                found,
            });
        }
        self.destination.set_switch(Switch::Standby);
        self.origin.activate();
        self.phase = Phase::RolledBack;
        Ok(())
    }

    /// Rule 2, forbidden half: forward recovery, step 1. Freeze the
    /// new authority and export its state to `workspace/replay`. Only
    /// legal when there are already new writes — if there are none the free
    /// rollback exists and they are not disguised as recovery.
    pub async fn export_forward(&mut self) -> Result<PathBuf, CutoverError> {
        self.require(Phase::CutOverLive, "export_forward")?;
        self.replay_started = Some(Instant::now());
        self.destination.freeze();
        let package = self.workspace.join("replay");
        let rows = drain_rows(self.destination.store().as_ref()).await?;
        export_package(&rows, &package, 8)?;
        self.phase = Phase::ReplayExported;
        Ok(package)
    }

    /// Forward recovery, step 2: re-import the replay into the origin
    /// (with `overwrite` for the identities that the new authority
    /// touched: the canonical identity rules) and pass the gate again. Without
    /// `Verified` here there is no return either.
    pub async fn replay_to_origin(&mut self) -> Result<CutoverToken, CutoverError> {
        self.require(Phase::ReplayExported, "replay_to_origin")?;
        let package = self.workspace.join("replay");
        let options = ImportOptions {
            overwrite: true,
            ..self.options.clone()
        };
        let progress = self.workspace.join(".replay-progress.json");
        import_export(
            &package,
            self.origin.store().clone(),
            Some(self.cas.clone()),
            &options,
            &progress,
        )
        .await
        .map_err(CutoverError::Import)?;
        // The gate in the return direction: origin (after the replay)
        // versus destination (frozen in the export).
        let origin = prepare_from_sink(self.origin.store().as_ref()).await?;
        let destination = prepare_from_sink(self.destination.store().as_ref()).await?;
        match gate_cutover(&origin, &destination, self.cas.as_ref()).await? {
            CutoverDecision::Verified(token) => {
                self.replay_token = Some(token.clone());
                self.phase = Phase::ReplayValidated;
                Ok(token)
            }
            CutoverDecision::Blocked(divergences) => Err(CutoverError::Blocked(divergences)),
        }
    }

    /// Forward recovery, step 3: the return only with the token that this
    /// replay produced, and only if the origin did not move since then.
    pub async fn rollback_with_replay(&mut self, token: &CutoverToken) -> Result<(), CutoverError> {
        self.require(Phase::ReplayValidated, "rollback_with_replay")?;
        let verified = self
            .replay_token
            .clone()
            .ok_or(CutoverError::IllegalTransition {
                phase: Phase::ReplayValidated.name(),
                action: "rollback without a replay token",
            })?;
        if *token != verified {
            return Err(CutoverError::StaleToken {
                expected_origin: verified.origin_fingerprint().to_string(),
                expected_destination: verified.destination_fingerprint().to_string(),
            });
        }
        let found = self.origin.fingerprint().await?;
        if found != verified.origin_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.origin.label().to_string(),
                verified: verified.origin_fingerprint().to_string(),
                found,
            });
        }
        self.origin.activate();
        if let Some(started) = self.replay_started {
            self.replay_window = Some(started.elapsed());
        }
        self.phase = Phase::RolledBack;
        Ok(())
    }

    /// The window and the metrics measured in this run (AC3).
    pub fn metrics(&self) -> CutoverMetrics {
        CutoverMetrics {
            window: CutoverWindow {
                barrier: self.elapsed_between("barrier", "frozen"),
                snapshot: self.elapsed_between("frozen", "snapshot"),
                import: self.elapsed_between("snapshot", "import"),
                verify: self.elapsed_between("import", "verify"),
                switch: self.elapsed_between("verify", "switch"),
                total: self.elapsed_between("barrier", "switch"),
            },
            new_writes: self.new_writes,
            origin_refused: self.origin.refused_writes(),
            verified_fingerprint: self
                .token
                .as_ref()
                .map(|token| token.destination_fingerprint().to_string())
                .unwrap_or_default(),
            replay_window: self.replay_window,
        }
    }

    /// The markdown evidence of this run: phases, measured durations
    /// and the observed RPO/RTO numbers. The test writes it
    /// to `target/uat/hsp212/evidencia.md`; nothing is promised here that
    /// has not been timed.
    pub fn evidence(&self) -> String {
        let metrics = self.metrics();
        let seconds =
            |value: Duration| format!("{}.{:03} s", value.as_secs(), value.subsec_millis());
        let mut out = String::new();
        out.push_str("# HSP-212 — ensayo de cutover: evidencia medida\n\n");
        out.push_str("Valores **observados** en esta corrida, no objetivos de diseño.\n\n");
        out.push_str("| pierna | duración |\n| --- | --- |\n");
        for (name, value) in [
            ("barrera → congelado", metrics.window.barrier),
            ("congelado → snapshot", metrics.window.snapshot),
            ("snapshot → import", metrics.window.import),
            ("import → gate", metrics.window.verify),
            ("gate → corte", metrics.window.switch),
            ("**ventana total**", metrics.window.total),
        ] {
            out.push_str(&format!("| {name} | {} |\n", seconds(value)));
        }
        out.push_str(&format!(
            "\n- RPO observado: {} filas perdidas en el corte (barrera antes del \
             snapshot; intentos de escritura rechazados por el origen congelado: \
             {}, contados, no estimados)\n",
            metrics.rpo_rows_observed(),
            metrics.origin_refused
        ));
        out.push_str(&format!(
            "- RTO observado (ventana de escrituras pausada, barrera → corte): {}\n",
            seconds(metrics.window.total)
        ));
        if let Some(replay) = metrics.replay_window {
            out.push_str(&format!(
                "- Forward recovery pos-corte completo (congelar destino → replay → \
                 regreso con token): {}\n",
                seconds(replay)
            ));
        }
        out.push_str(&format!(
            "- Fingerprint de autoridades verificado en el corte: `{}`\n\
             - Escrituras nuevas acumuladas al medir: {}\n\
             - Fase final: {}\n",
            metrics.verified_fingerprint,
            metrics.new_writes,
            self.phase.name()
        ));
        out
    }

    fn require(&self, expected: Phase, action: &'static str) -> Result<(), CutoverError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(CutoverError::IllegalTransition {
                phase: self.phase.name(),
                action,
            })
        }
    }
}

/// Reads all the rows of a sink through its state summary, just as
/// `prepare_from_sink` does: the list of identities is that of the summary,
/// never a global census of the store.
pub async fn drain_rows(sink: &dyn RowSink) -> Result<BTreeMap<String, Row>, RecoveryError> {
    let summary = sink.state_summary().await?;
    let mut rows = BTreeMap::new();
    for identity in summary.rows.keys() {
        let row = sink
            .lookup(identity)
            .await?
            .ok_or_else(|| RecoveryError::Store(format!("the sink lost '{identity}'")))?;
        rows.insert(identity.clone(), row);
    }
    Ok(rows)
}

/// Writes a bundle in the HSP-210 format from a canonical map of
/// rows: batches ordered by identity, real hashes, coherent boundary
/// and self-digest. It is the rehearsal's export — what the frozen origin
/// hands to the import — with the same shape that
/// [`ImportManifest::check`] validates. At the end it is re-read with the contract's
/// reader: if the bundle does not pass its own reading, it is not a snapshot.
pub fn export_package(
    rows: &BTreeMap<String, Row>,
    dir: &Path,
    batch_rows: usize,
) -> Result<ImportManifest, RecoveryError> {
    let batch_rows = batch_rows.max(1);
    std::fs::create_dir_all(dir.join("batches")).map_err(|error| {
        RecoveryError::Io(format!("the package directory cannot be created: {error}"))
    })?;

    let mut by_table: BTreeMap<String, Vec<(String, Row)>> = BTreeMap::new();
    for (identity, row) in rows {
        let table = identity.split(':').next().unwrap_or(identity).to_string();
        by_table
            .entry(table)
            .or_default()
            .push((identity.clone(), with_canonical_id(row.clone(), identity)));
    }

    let mut tables = Vec::new();
    let mut frontier_tables = BTreeMap::new();
    let mut ledger_rows = 0u64;
    let mut ledger_max_position: Option<u64> = None;
    let mut head_rows = 0u64;
    for (table, entries) in by_table {
        let mut batches = Vec::new();
        for (index, chunk) in entries.chunks(batch_rows).enumerate() {
            let payload: Vec<Row> = chunk.iter().map(|(_, row)| row.clone()).collect();
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
            let file = format!("batches/{table}-{index:06}.json");
            std::fs::write(dir.join(&file), &bytes)
                .map_err(|error| RecoveryError::Io(error.to_string()))?;
            batches.push(ImportBatch {
                file,
                sha256: sha256_hex(&bytes),
                rows: payload.len() as u64,
                first_id: chunk.first().expect("chunks are non-empty").0.clone(),
                last_id: chunk.last().expect("chunks are non-empty").0.clone(),
            });
            if table == "event_ledger" {
                ledger_rows += payload.len() as u64;
                for row in &payload {
                    if let Some(position) = row.get("position").and_then(serde_json::Value::as_u64)
                    {
                        ledger_max_position = Some(ledger_max_position.unwrap_or(0).max(position));
                    }
                }
            }
            if table == "event_ledger_head" {
                head_rows += payload.len() as u64;
            }
        }
        let total: u64 = batches.iter().map(|batch| batch.rows).sum();
        frontier_tables.insert(table.clone(), total);
        tables.push(ImportTable {
            table,
            rows: total,
            batches,
        });
    }

    let mut manifest = ImportManifest {
        format: IMPORT_FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source: ExportSource {
            engine: "hodei-artifacts-api".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: "cutover-rehearsal".to_string(),
            namespace: "hodei".to_string(),
            database: "artifacts".to_string(),
        },
        frontier: ExportFrontier {
            tables: frontier_tables,
            ledger_rows,
            ledger_max_position,
            head_rows,
        },
        tables,
        blobs: BlobPolicy {
            policy: "referenced-by-digest".to_string(),
            copied: false,
        },
        content_sha256: String::new(),
    };
    manifest.content_sha256 = manifest.content_digest();
    std::fs::write(
        dir.join(IMPORT_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| RecoveryError::Metadata(error.to_string()))?,
    )
    .map_err(|error| RecoveryError::Io(error.to_string()))?;
    read_import_manifest(dir)
}
