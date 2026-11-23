//! The shared transactional guard per scope: the COMMON conflict key of
//! every writer that touches tags, references, GC holds or quotas.
//!
//! Why a COMMON guard, and not one invented per caller: snapshot
//! isolation is not serialization. Two transactions can both read the
//! same state and both commit unless the server forces a conflict
//! between them. The guard is the forcing function: every operation
//! that belongs to a scope advances that scope's ONE guard node
//! (`hodei.guard.key`, exact-indexed and `@upsert`) as part of its own
//! transaction, and the `@upsert` index makes two concurrent writers of
//! the same scope abort at the server. The conflict key derives from
//! the scope alone — kind, tenant, subject — never from the caller, so
//! every writer of the same scope collides on the same node and
//! writers of different scopes never do.
//!
//! The guard doubles as the CAS anchor of the scope: an operation can
//! demand that the scope be at an exact revision ("confirm this tag
//! only if it is still at revision r"). Two writers demanding the same
//! revision cannot both confirm: one wins the server-side race, the
//! other revalidates and reports the rejection with the current
//! revision. Success is decided by the state after the commit, never
//! inferred from the transaction merely having committed.
//!
//! With [`crate::idempotency`], the whole write is once-only: the
//! operation marker commits atomically with the effect and the guard
//! advance, so a lost ACK or a partial failure replays the committed
//! result instead of duplicating the effect. The retry loop is the
//! doc-20 prescription: every attempt revalidates (a fresh read decides
//! the CAS), conflicts retry with bounded backoff, and exhaustion is a
//! typed closed failure — never a silent fallback, never an unbounded
//! retry, never a swallowed error.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::debug;

use crate::client::GraphClient;
use crate::error::GraphClientError;
use crate::idempotency::{OPERATION_TYPE, OperationId, OperationRecord};
use crate::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use crate::schema::{SchemaBootError, ensure_ready};

/// The type tag of guard nodes in the graph.
pub const GUARD_TYPE: &str = "HodeiGuard";

/// The separator of the composite scope key. Both the tenant and the
/// subject are validated to exclude it, so the key has one parsing.
pub const SCOPE_SEPARATOR: char = crate::identity::KEY_SEPARATOR;

/// How many attempts a guarded write may lose before giving up. The
/// bound is deliberately higher than the identity upserts': the guarded
/// write races against EVERY concurrent writer of the scope, and a
/// burst of N contenders needs up to N serialized rounds before every
/// caller has won its slot. A caller that exhausts the bound can
/// SAFELY re-drive the same operation id (the registry replays what
/// committed), so the bound sheds load without risking duplicates.
const MAX_ATTEMPTS: usize = 12;

/// Base delay of the retry backoff; it doubles per attempt.
const RETRY_BASE: Duration = Duration::from_millis(10);

/// The backoff ceiling: under a wide race the exponential schedule
/// would outgrow the caller's patience, so the delay stops growing
/// here and the jitter does the desynchronization.
const RETRY_CAP: Duration = Duration::from_millis(100);

/// The maximum length of a scope subject.
const MAX_SUBJECT_LEN: usize = 256;

/// What kind of aggregate the scope guards. The kind is part of the
/// conflict key, so a tag race and a reference race never collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardKind {
    /// Tag writes over one tag subject.
    Tag,
    /// Reference writes over one reference subject.
    Reference,
    /// GC holds over one garbage-collection subject.
    GcHold,
    /// Quota reservations over one quota subject.
    Quota,
}

impl GuardKind {
    /// The kind's text in the conflict key.
    pub fn as_str(self) -> &'static str {
        match self {
            GuardKind::Tag => "tag",
            GuardKind::Reference => "reference",
            GuardKind::GcHold => "gc-hold",
            GuardKind::Quota => "quota",
        }
    }
}

/// The validated scope a guard protects: kind, tenant and subject. The
/// scope IS the conflict key's derivation input, and nothing else is:
/// two callers that build the same scope — from whatever code path —
/// share the same guard node, which is what makes the guard common.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuardScope {
    kind: GuardKind,
    tenant: crate::identity::Tenant,
    subject: String,
}

impl GuardScope {
    /// Validates and builds a scope. The subject must be non-empty
    /// ASCII with the safe charset of an HRN-ish path (alphanumeric,
    /// `-`, `_`, `.`, `/`, `:`, `@`), never the scope separator and
    /// never whitespace or control characters.
    pub fn new(
        kind: GuardKind,
        tenant: &crate::identity::Tenant,
        subject: &str,
    ) -> Result<Self, GuardError> {
        let valid = !subject.is_empty()
            && subject.len() <= MAX_SUBJECT_LEN
            && subject.starts_with(|c: char| c.is_ascii_alphanumeric())
            && subject.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@')
            })
            && !subject.contains(SCOPE_SEPARATOR);
        if !valid {
            return Err(GuardError::InvalidScope {
                scope: format!(
                    "{}{}{}{}{}",
                    kind.as_str(),
                    SCOPE_SEPARATOR,
                    tenant.as_str(),
                    SCOPE_SEPARATOR,
                    subject
                ),
                reason: "the subject must be non-empty alphanumeric with '-', '_', '.', '/', ':' and '@', without the scope separator",
            });
        }
        Ok(Self {
            kind,
            tenant: tenant.clone(),
            subject: subject.to_owned(),
        })
    }

    /// The kind of aggregate this scope guards.
    pub fn kind(&self) -> GuardKind {
        self.kind
    }

    /// The tenant of the scope.
    pub fn tenant(&self) -> &crate::identity::Tenant {
        &self.tenant
    }

    /// The subject of the scope.
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

/// The composite conflict key of a scope: `kind|tenant|subject`. THE
/// shared key: every writer of the same scope, from any code path,
/// derives this exact string and therefore collides on the same guard
/// node.
pub fn guard_key(scope: &GuardScope) -> String {
    format!(
        "{}{}{}{}{}",
        scope.kind().as_str(),
        SCOPE_SEPARATOR,
        scope.tenant().as_str(),
        SCOPE_SEPARATOR,
        scope.subject()
    )
}

/// Whether the guarded write must advance the scope revision freely or
/// demand an exact one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionDemand {
    /// Advance whatever the current revision is. Concurrent writers of
    /// the scope serialize through the guard: each retry revalidates
    /// and applies on top of whatever committed meanwhile.
    Advance,
    /// Commit only if the guard is exactly at this revision (CAS). Two
    /// writers demanding the same revision cannot both confirm: the
    /// server decides, the loser reports [`GuardError::CasRejected`]
    /// with the current revision.
    At(u64),
}

/// The effect a guarded operation applies inside the guarded
/// transaction. The executor wraps it: the effect commits atomically
/// with the guard advance and the operation marker, or not at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardedEffect {
    /// Extra DQL query lines binding the uid variables the N-Quads
    /// reference (one statement per line, no braces). The names `g`,
    /// `op`, `ops` and the blank nodes `_:guard`, `_:opmarker` are
    /// reserved by the executor.
    pub query_lines: Vec<String>,
    /// The N-Quads of the effect, applied in the guarded block.
    pub set_nquads: String,
    /// The result to record on the operation node: what a replay
    /// returns as the already-committed answer.
    pub recorded_result: String,
}

/// What a guarded operation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedOutcome {
    /// The scope key the operation ran under.
    pub scope: String,
    /// The operation id.
    pub operation_id: String,
    /// The guard revision AFTER the operation (read back from the
    /// server, never inferred).
    pub guard_revision: u64,
    /// True when the operation had already committed (a replay): no
    /// new effect was applied, [`Self::recorded_result`] is the
    /// committed answer.
    pub replayed: bool,
    /// The recorded result of the operation.
    pub recorded_result: String,
}

/// Why a guarded operation failed. Every variant is a closed failure:
/// the caller knows exactly what happened and nothing was guessed.
#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    /// The schema boot refused the server: the store never opens on an
    /// incompatible graph, so no guarded write can land on it.
    #[error("the store cannot open on this server: {0}")]
    Schema(#[from] SchemaBootError),
    /// A transport, auth or protocol failure, including the
    /// `UpsertConflict` that retryable loops handle internally.
    #[error(transparent)]
    Graph(#[from] GraphClientError),
    /// The operation registry rejected the operation id or found a
    /// record that contradicts the contract.
    #[error(transparent)]
    Operation(#[from] crate::idempotency::OperationError),
    /// The scope is not a safe conflict key.
    #[error("the scope '{scope}' is not valid: {reason}")]
    InvalidScope {
        /// The scope as received.
        scope: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// The CAS demand was rejected: someone else advanced the scope
    /// first. The caller reads the current revision from the error and
    /// decides (retry with the new revision, or stop).
    #[error(
        "the operation on '{scope}' lost the CAS check: expected revision {expected_revision} \
         but the scope is at revision {current_revision}"
    )]
    CasRejected {
        /// The scope key.
        scope: String,
        /// The revision the caller demanded.
        expected_revision: u64,
        /// The revision the scope actually has.
        current_revision: u64,
    },
    /// The graph contradicts its own guard contract (duplicate guard
    /// nodes, a committed effect without its marker, ...): a real
    /// invariant break, never repaired silently.
    #[error("the guarded state of '{scope}' contradicts the guard contract: {detail}")]
    CorruptState {
        /// The scope key involved.
        scope: String,
        /// What contradicts the contract.
        detail: String,
    },
    /// The guarded write lost every attempt. Bounded, explicit, and
    /// always reported: success is never inferred from exhaustion.
    #[error(
        "the guarded operation on '{scope}' did not commit after {attempts} attempts; \
         last failure: {last}"
    )]
    RetryExhausted {
        /// The scope key being written.
        scope: String,
        /// How many attempts were made.
        attempts: usize,
        /// The last underlying failure.
        last: String,
    },
}

/// The retry decision of the guarded write loop. Pure, so the bounds
/// are testable without a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptDecision {
    /// Retry after this delay.
    RetryAfter(Duration),
    /// Give up with the typed exhaustion error.
    Exhausted,
}

/// The retry policy: only the server-forced conflict is retryable, and
/// only up to [`MAX_ATTEMPTS`], with doubling backoff. Any other
/// failure surfaces immediately — retrying a transport or protocol
/// failure blindly would hide it. Attempt numbers are 1-based; after
/// the last attempt the decision is exhaustion.
pub(crate) fn decide_retry(attempt: usize, conflict: bool) -> AttemptDecision {
    if !conflict || attempt >= MAX_ATTEMPTS {
        return AttemptDecision::Exhausted;
    }
    let exponential = RETRY_BASE * (1u32 << (attempt - 1));
    AttemptDecision::RetryAfter(exponential.min(RETRY_CAP))
}

/// A small jitter on top of the policy delay: concurrent losers of one
/// serialization round must not retry in lockstep, or every round
/// frees at most one slot and the race degenerates into waves.
fn jittered(delay: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    delay + Duration::from_millis((nanos % 21) as u64)
}

/// The CAS decision: which revision the guard moves to, or the pair
/// `(expected, current)` that the rejection reports. Pure and
/// deterministic: this is the decision that keeps two writers from
/// confirming the same revision of a scope. An absent guard reads as
/// revision 0, so `At(0)` claims a fresh scope.
pub(crate) fn decide_next_revision(
    demand: RevisionDemand,
    current: u64,
) -> Result<u64, (u64, u64)> {
    match demand {
        RevisionDemand::Advance => Ok(current + 1),
        RevisionDemand::At(expected) if expected == current => Ok(current + 1),
        RevisionDemand::At(expected) => Err((expected, current)),
    }
}

/// The persistent guard store. Open through [`GuardStore::open`], which
/// runs the schema boot first: an incompatible server is refused before
/// any write path exists. Its `Debug` names the boot decision and
/// nothing else.
#[derive(Debug)]
pub struct GuardStore {
    client: Arc<GraphClient>,
    boot: crate::schema::SchemaBootReport,
}

impl GuardStore {
    /// Opens the store on a connected client, gating on the versioned
    /// schema. This is the readiness boundary: no store exists on an
    /// incompatible server, hence no guarded write either.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, GuardError> {
        let boot = ensure_ready(&client).await?;
        tracing::info!(
            decision = ?boot.decision,
            binary_version = boot.binary_version,
            verified_predicates = boot.verified_predicates,
            "the guard store opened on a validated schema"
        );
        Ok(Self { client, boot })
    }

    /// The schema boot report this store validated at open.
    pub fn boot_report(&self) -> &crate::schema::SchemaBootReport {
        &self.boot
    }

    /// Runs one guarded operation: consult the idempotency registry,
    /// decide the CAS from a fresh read, and apply effect + guard
    /// advance + operation marker in ONE transaction that the server
    /// itself forces into conflict with any concurrent writer of the
    /// same scope. The transaction REVALIDATES the expected revision
    /// inside its own snapshot (the advance block is gated on the
    /// in-transaction revision being exactly the pre-read one), so a
    /// pre-read overtaken by a concurrent commit is detected and
    /// retried instead of silently writing a stale value. A committed
    /// operation id replays its recorded result; a conflicted or
    /// skipped write retries bounded with revalidation; exhaustion,
    /// CAS rejection and corruption are typed closed failures.
    pub async fn execute_guarded(
        &self,
        scope: &GuardScope,
        operation: &OperationId,
        demand: RevisionDemand,
        effect: GuardedEffect,
    ) -> Result<GuardedOutcome, GuardError> {
        let key = guard_key(scope);
        let mut last_failure = "no detail".to_owned();
        for attempt in 1..=MAX_ATTEMPTS {
            // Revalidation: a fresh read of the scope decides the CAS,
            // and the operation registry is consulted in the SAME round
            // trip (one narrower window per attempt). The read is a
            // hint, not the correctness mechanism: the transaction
            // below re-checks it against its own snapshot and the
            // token's @upsert index forces the conflict, so a read
            // overtaken by a concurrent commit costs one retry, never
            // a lost update.
            let (current, existing) = self.read_scope_state(scope, operation).await?;
            // Idempotency BEFORE applying: the same operation id that
            // already committed replays its recorded result, whatever
            // the demand says (the demand would apply a NEW effect,
            // and this operation is not new).
            if let Some(record) = existing {
                return Ok(GuardedOutcome {
                    scope: key,
                    operation_id: operation.as_str().to_owned(),
                    guard_revision: current,
                    replayed: true,
                    recorded_result: crate::idempotency::replayed_result(Some(&record))
                        .unwrap_or_default(),
                });
            }
            // The CAS demand is decided here (typed rejection with the
            // current revision); the advance itself revalidates inside
            // the transaction below.
            if let Err((expected, current)) = decide_next_revision(demand, current) {
                return Err(GuardError::CasRejected {
                    scope: key,
                    expected_revision: expected,
                    current_revision: current,
                });
            }
            let transaction = guarded_upsert(&key, operation.as_str(), current, &effect);
            match self.client.upsert(transaction).await {
                Ok(response) => {
                    // The response carries the query results evaluated
                    // inside the committing transaction.
                    let from_response = crate::idempotency::parse_operation(
                        &response.json,
                        "ops",
                        operation.as_str(),
                    )?;
                    if let Some(record) = from_response {
                        // The marker existed inside the transaction:
                        // ANOTHER transaction committed this very
                        // operation — the once-only contract replayed.
                        let revision = self.guard_revision(scope).await?;
                        return Ok(GuardedOutcome {
                            scope: key,
                            operation_id: operation.as_str().to_owned(),
                            guard_revision: revision,
                            replayed: true,
                            recorded_result: record.result,
                        });
                    }
                    // No marker in the transaction snapshot: the block
                    // applied only if the in-transaction guard state
                    // still matched the pre-read revision. A skipped
                    // block committed nothing — revalidate, never
                    // infer success.
                    let applied = guard_block_applied(&key, &response.json, current)?;
                    if !applied {
                        debug!(
                            attempt,
                            scope = %key,
                            expected = current,
                            "the guarded block was skipped: the scope moved under the pre-read; revalidating"
                        );
                        last_failure = format!(
                            "the guarded write was skipped: the scope moved under the pre-read \
                             (expected revision {current})"
                        );
                        if let AttemptDecision::RetryAfter(delay) = decide_retry(attempt, true) {
                            sleep(jittered(delay)).await;
                            continue;
                        }
                        break;
                    }
                    // This attempt committed the effect. The marker
                    // must exist now: its absence is corruption.
                    let record = self.find_operation(operation).await?.ok_or_else(|| {
                        GuardError::CorruptState {
                            scope: key.clone(),
                            detail: "the transaction committed but the operation marker is missing"
                                .to_owned(),
                        }
                    })?;
                    // The state after the commit decides the answer.
                    let revision = self.guard_revision(scope).await?;
                    return Ok(GuardedOutcome {
                        scope: key,
                        operation_id: operation.as_str().to_owned(),
                        guard_revision: revision,
                        replayed: false,
                        recorded_result: record.result,
                    });
                }
                Err(GraphClientError::UpsertConflict { detail }) => {
                    debug!(attempt, scope = %key, %detail, "the guarded write conflicted; revalidating");
                    last_failure = detail;
                    if let AttemptDecision::RetryAfter(delay) = decide_retry(attempt, true) {
                        sleep(jittered(delay)).await;
                        continue;
                    }
                    break;
                }
                Err(other) => return Err(other.into()),
            }
        }
        Err(GuardError::RetryExhausted {
            scope: key,
            attempts: MAX_ATTEMPTS,
            last: last_failure,
        })
    }

    /// Reads the scope's guard revision AND the operation's committed
    /// record in ONE round trip: the pre-read of every attempt. Both
    /// values are hints (the transaction revalidates the revision), but
    /// one narrow read keeps the retry storm small under contention.
    async fn read_scope_state(
        &self,
        scope: &GuardScope,
        operation: &OperationId,
    ) -> Result<(u64, Option<OperationRecord>), GuardError> {
        let key = guard_key(scope);
        let query = format!(
            "{{ op(func: eq(hodei.operation.id, {})) {{ hodei.operation.id hodei.operation.scope \
             hodei.operation.result hodei.operation.committed_at }}\n \
             guard(func: eq(hodei.guard.key, {})) {{ hodei.guard.revision }} }}",
            crate::schema::dql_string(operation.as_str()),
            crate::schema::dql_string(&key),
        );
        let response = self.client.execute_dql(&query).await?;
        let record = crate::idempotency::parse_operation(&response.json, "op", operation.as_str())?;
        let nodes = response
            .json
            .get("guard")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let revision = match nodes.len() {
            0 => 0,
            1 => nodes[0]
                .get("hodei.guard.revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            n => {
                return Err(GuardError::CorruptState {
                    scope: key,
                    detail: format!("{n} guard nodes carry the same scope key"),
                });
            }
        };
        Ok((revision, record))
    }

    /// The current revision of a scope's guard. An absent guard is
    /// revision 0: the scope has never been written. Duplicate guard
    /// nodes are corruption.
    pub async fn guard_revision(&self, scope: &GuardScope) -> Result<u64, GuardError> {
        let key = guard_key(scope);
        let query = format!(
            "{{ q(func: eq(hodei.guard.key, {})) {{ uid hodei.guard.revision }} }}",
            crate::schema::dql_string(&key),
        );
        let response = self.client.execute_dql(&query).await?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(0),
            1 => Ok(nodes[0]
                .get("hodei.guard.revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)),
            n => Err(GuardError::CorruptState {
                scope: key,
                detail: format!("{n} guard nodes carry the same scope key"),
            }),
        }
    }

    /// Looks up the committed record of an operation id. `None` when
    /// the operation has never committed.
    pub async fn find_operation(
        &self,
        operation: &OperationId,
    ) -> Result<Option<OperationRecord>, GuardError> {
        let query = format!(
            "{{ q(func: eq(hodei.operation.id, {})) {{ hodei.operation.id hodei.operation.scope \
             hodei.operation.result hodei.operation.committed_at }} }}",
            crate::schema::dql_string(operation.as_str()),
        );
        let response = self.client.execute_dql(&query).await?;
        crate::idempotency::parse_operation(&response.json, "q", operation.as_str())
            .map_err(GuardError::from)
    }
}

/// The guarded write: find the guard node by the scope's conflict key,
/// revalidate the expected revision INSIDE the transaction's own
/// snapshot (`gv` binds the guard only when its revision is still the
/// pre-read one), and apply — in ONE atomic transaction — the guard
/// advance, the operation marker and the caller's effect, gated on the
/// operation not having committed already.
///
/// The conflict anchor is the scope-qualified token of the next
/// revision (`hodei.guard.token`, exact-indexed and `@upsert`): two
/// writers advancing to the same revision write the SAME new token, so
/// the server aborts one of them; a writer whose pre-read was overtaken
/// writes NOTHING (the `gv` gate fails) and retries after a fresh read.
/// Writing an already-present value is a no-op on the server, which is
/// exactly why the stale value must never be written: snapshot
/// isolation is not serialization, so the conflict is forced, not
/// hoped for.
fn guarded_upsert(
    key: &str,
    operation_id: &str,
    expected_revision: u64,
    effect: &GuardedEffect,
) -> GraphUpsertTransaction {
    let key_literal = crate::schema::dql_string(key);
    let id_literal = crate::schema::dql_string(operation_id);
    let result_literal = crate::schema::dql_string(&effect.recorded_result);
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let next_revision = expected_revision + 1;
    let token_literal = crate::schema::dql_string(&format!("{key}#{next_revision}"));
    // The query binds exactly the variables its branch uses (DQL
    // refuses unused variables): the create branch needs the unfiltered
    // guard lookup, the advance branch needs the in-transaction
    // revalidation filter. Both carry the operation lookup, the
    // readable ops block and the in-transaction guard state the caller
    // decides from.
    let guard_lookup = if expected_revision == 0 {
        format!("g as var(func: eq(hodei.guard.key, {key_literal}))")
    } else {
        format!(
            "gv as var(func: eq(hodei.guard.key, {key_literal})) \
             @filter(eq(hodei.guard.revision, {expected_revision}))"
        )
    };
    let mut query = format!(
        "{{ {guard_lookup}\n \
             op as var(func: eq(hodei.operation.id, {id_literal}))\n \
             ops(func: eq(hodei.operation.id, {id_literal})) {{ hodei.operation.id \
             hodei.operation.scope hodei.operation.result hodei.operation.committed_at }}\n \
             guard(func: eq(hodei.guard.key, {key_literal})) {{ hodei.guard.revision }}\n"
    );
    for line in &effect.query_lines {
        query.push_str("     ");
        query.push_str(line.trim_start());
        query.push('\n');
    }
    query.push('}');
    // The marker and the effect commit exactly when the operation
    // commits, atomically with the guard advance.
    let marker_and_effect = format!(
        "_:opmarker <dgraph.type> \"{OPERATION_TYPE}\" .\n\
         _:opmarker <hodei.operation.id> {id_literal} .\n\
         _:opmarker <hodei.operation.scope> {key_literal} .\n\
         _:opmarker <hodei.operation.result> {result_literal} .\n\
         _:opmarker <hodei.operation.committed_at> \"{now}\" .\n\
         {}",
        effect.set_nquads
    );
    // Fresh scope (expected 0 = the pre-read found no guard): create
    // the guard node at revision 1. Two concurrent creators of the same
    // scope write the same guard key AND the same first token: the
    // `@upsert` indexes abort one of them — exactly one guard node per
    // scope, ever.
    let blocks = if expected_revision == 0 {
        vec![GraphMutationBlock::set_if(
            "eq(len(op), 0) AND eq(len(g), 0)",
            format!(
                "_:guard <dgraph.type> \"{GUARD_TYPE}\" .\n\
                 _:guard <hodei.guard.key> {key_literal} .\n\
                 _:guard <hodei.guard.revision> \"{next_revision}\" .\n\
                 _:guard <hodei.guard.token> {token_literal} .\n\
                 _:guard <hodei.guard.updated_at> \"{now}\" .\n\
                 {marker_and_effect}"
            ),
        )]
    } else {
        // Existing scope: advance THE shared guard node, gated on the
        // in-transaction revision being still the expected one. Two
        // writers revalidated the same revision write the same token:
        // the server decides who advances the scope.
        vec![GraphMutationBlock::set_if(
            "eq(len(op), 0) AND eq(len(gv), 1)",
            format!(
                "uid(gv) <hodei.guard.revision> \"{next_revision}\" .\n\
                 uid(gv) <hodei.guard.token> {token_literal} .\n\
                 uid(gv) <hodei.guard.updated_at> \"{now}\" .\n\
                 {marker_and_effect}"
            ),
        )]
    };
    GraphUpsertTransaction {
        query,
        blocks,
        commit_now: true,
    }
}

/// Whether the guarded block actually applied, decided from the
/// transaction's own answer: the `guard` named block carries the
/// in-transaction guard state, so a pre-read overtaken by a concurrent
/// commit is detected as a skipped block (retry), never as success.
/// An `expected_revision` of 0 committed only if the guard was absent
/// inside the transaction.
fn guard_block_applied(
    key: &str,
    json: &serde_json::Value,
    expected_revision: u64,
) -> Result<bool, GuardError> {
    let nodes = json
        .get("guard")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if expected_revision == 0 {
        // The create branch applies only while the guard is absent in
        // the transaction's snapshot.
        return Ok(nodes.is_empty());
    }
    match nodes.len() {
        0 => Ok(false),
        1 => {
            let in_tx = nodes[0]
                .get("hodei.guard.revision")
                .and_then(serde_json::Value::as_u64);
            Ok(in_tx == Some(expected_revision))
        }
        n => Err(GuardError::CorruptState {
            scope: key.to_owned(),
            detail: format!("{n} guard nodes answered the in-transaction check"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Tenant;

    fn tenant() -> Tenant {
        Tenant::new("tenant-a").expect("valid tenant")
    }

    fn scope(subject: &str) -> GuardScope {
        GuardScope::new(GuardKind::Tag, &tenant(), subject).expect("valid scope")
    }

    /// The conflict key derives from the scope alone: two callers that
    /// build the same scope — from whatever code path — derive the same
    /// key and therefore share one guard node. A different tenant,
    /// kind or subject derives a different key and never collides.
    #[test]
    fn the_scope_key_is_derived_from_the_scope_alone() {
        let a =
            GuardScope::new(GuardKind::Tag, &tenant(), "repo/artifact@v1").expect("valid scope");
        let b =
            GuardScope::new(GuardKind::Tag, &tenant(), "repo/artifact@v1").expect("valid scope");
        assert_eq!(
            guard_key(&a),
            guard_key(&b),
            "same scope, same conflict key"
        );
        assert_eq!(guard_key(&a), "tag|tenant-a|repo/artifact@v1");
        let other_tenant = GuardScope::new(
            GuardKind::Tag,
            &Tenant::new("tenant-b").expect("valid tenant"),
            "repo/artifact@v1",
        )
        .expect("valid scope");
        let other_kind = GuardScope::new(GuardKind::Reference, &tenant(), "repo/artifact@v1")
            .expect("valid scope");
        let other_subject = scope("repo/artifact@v2");
        assert_ne!(guard_key(&a), guard_key(&other_tenant));
        assert_ne!(guard_key(&a), guard_key(&other_kind));
        assert_ne!(guard_key(&a), guard_key(&other_subject));
    }

    /// Every declared kind renders its own key segment.
    #[test]
    fn every_kind_renders_its_own_segment() {
        let subject = "subject";
        assert_eq!(
            guard_key(&GuardScope::new(GuardKind::Tag, &tenant(), subject).expect("valid")),
            "tag|tenant-a|subject"
        );
        assert_eq!(
            guard_key(&GuardScope::new(GuardKind::Reference, &tenant(), subject).expect("valid")),
            "reference|tenant-a|subject"
        );
        assert_eq!(
            guard_key(&GuardScope::new(GuardKind::GcHold, &tenant(), subject).expect("valid")),
            "gc-hold|tenant-a|subject"
        );
        assert_eq!(
            guard_key(&GuardScope::new(GuardKind::Quota, &tenant(), subject).expect("valid")),
            "quota|tenant-a|subject"
        );
    }

    /// A subject that could break the conflict key is rejected before
    /// any server call.
    #[test]
    fn subjects_that_could_break_the_key_are_rejected() {
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), "").is_err());
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), "a|b").is_err());
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), "a b").is_err());
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), "-nope").is_err());
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), "ña").is_err());
        let long = "a".repeat(MAX_SUBJECT_LEN + 1);
        assert!(GuardScope::new(GuardKind::Tag, &tenant(), &long).is_err());
        let error = GuardScope::new(GuardKind::Tag, &tenant(), "bad|subject")
            .expect_err("the separator must be rejected");
        assert!(
            matches!(error, GuardError::InvalidScope { .. }),
            "unexpected: {error}"
        );
    }

    /// The CAS decision: an exact demand at the current revision moves
    /// to the next; an exact demand at any other revision is rejected
    /// with both revisions; an advance always moves to the next.
    #[test]
    fn the_cas_decision_rejects_a_stale_revision() {
        assert_eq!(decide_next_revision(RevisionDemand::At(5), 5), Ok(6));
        assert_eq!(
            decide_next_revision(RevisionDemand::At(5), 6),
            Err((5, 6)),
            "a stale demand is rejected with the current revision"
        );
        assert_eq!(decide_next_revision(RevisionDemand::Advance, 0), Ok(1));
        assert_eq!(
            decide_next_revision(RevisionDemand::At(0), 0),
            Ok(1),
            "At(0) claims a fresh scope"
        );
    }

    /// Two writers demanding the SAME revision of the same scope get
    /// the same decision input; after the winner advances, the loser's
    /// fresh read can no longer satisfy the demand. The decision itself
    /// is what forces exactly one confirmation.
    #[test]
    fn the_same_revision_demanded_twice_cannot_confirm_twice() {
        // Both revalidated revision 2, both decide to move to 3.
        assert_eq!(decide_next_revision(RevisionDemand::At(2), 2), Ok(3));
        assert_eq!(decide_next_revision(RevisionDemand::At(2), 2), Ok(3));
        // The loser revalidates after the winner committed: revision 3.
        assert_eq!(
            decide_next_revision(RevisionDemand::At(2), 3),
            Err((2, 3)),
            "the loser reports the rejection, never a second confirmation"
        );
    }

    /// The retry policy is bounded: the server-forced conflict retries
    /// with doubling backoff up to the attempt limit, and exhaustion is
    /// the decision at the limit.
    #[test]
    fn the_retry_policy_is_bounded_and_backs_off() {
        let expected_backoffs_ms = [10, 20, 40, 80, 100, 100, 100, 100, 100, 100, 100];
        for (index, backoff_ms) in expected_backoffs_ms.iter().enumerate() {
            assert_eq!(
                decide_retry(index + 1, true),
                AttemptDecision::RetryAfter(Duration::from_millis(*backoff_ms)),
                "attempt {} must retry after {backoff_ms} ms",
                index + 1
            );
        }
        assert_eq!(
            decide_retry(MAX_ATTEMPTS, true),
            AttemptDecision::Exhausted,
            "after the last attempt the failure is closed, never a silent retry"
        );
    }

    /// A non-conflict failure is never retried: it surfaces immediately
    /// as a typed error (closed failure).
    #[test]
    fn non_conflict_failures_are_never_retried() {
        assert_eq!(decide_retry(1, false), AttemptDecision::Exhausted);
        assert_eq!(
            decide_retry(1, true),
            AttemptDecision::RetryAfter(Duration::from_millis(10))
        );
    }

    /// The guarded transaction carries the once-only gate (`len(op)`),
    /// the in-transaction revalidation of the expected revision (`gv`
    /// filtered by the revision), the scope-qualified conflict token of
    /// the next revision, and the caller's effect inside the guarded
    /// block. The advance branch is chosen exactly when the pre-read
    /// revision is not 0; the create branch only for a fresh scope.
    #[test]
    fn the_guarded_upsert_gates_on_the_operation_and_forces_the_conflict() {
        let effect = GuardedEffect {
            query_lines: vec!["e as var(func: eq(hodei.identity.name, \"target\"))".to_owned()],
            set_nquads: "uid(e) <hodei.identity.owner> _:ref .\n".to_owned(),
            recorded_result: "committed".to_owned(),
        };
        let advance = guarded_upsert("tag|tenant-a|t@1", "op-1", 4, &effect);
        assert!(
            advance.commit_now,
            "the guarded write commits now: the answer decides"
        );
        assert_eq!(
            advance.blocks.len(),
            1,
            "one attempt runs exactly one guarded block"
        );
        let block = &advance.blocks[0];
        let cond = block.cond.as_deref().expect("the guarded block is gated");
        assert!(
            cond.contains("eq(len(op), 0)"),
            "the block must be gated on the operation not existing: {cond}"
        );
        assert!(
            cond.contains("eq(len(gv), 1)"),
            "the advance block must be gated on the IN-TRANSACTION revision matching the pre-read: {cond}"
        );
        // The revalidation filter inside the query: a pre-read
        // overtaken by a concurrent commit writes nothing.
        assert!(
            advance
                .query
                .contains("gv as var(func: eq(hodei.guard.key, \"tag|tenant-a|t@1\")) @filter(eq(hodei.guard.revision, 4))"),
            "the query must revalidate the expected revision inside the transaction: {}",
            advance.query
        );
        // The conflict anchor: the scope-qualified token of the NEXT
        // revision, a NEW exact-index token two concurrent writers share.
        assert!(
            block
                .set_nquads
                .contains("uid(gv) <hodei.guard.token> \"tag|tenant-a|t@1#5\""),
            "the advance must write the scope-qualified next-revision token: {}",
            block.set_nquads
        );
        assert!(
            block
                .set_nquads
                .contains("uid(gv) <hodei.guard.revision> \"5\""),
            "the advance must move the shared guard node"
        );
        // The marker and the effect ride in the block.
        assert!(
            block.set_nquads.contains("hodei.operation.id"),
            "the marker must be there"
        );
        assert!(
            block.set_nquads.contains("uid(e)"),
            "the effect must be there"
        );
        // The query binds the operation and the readable ops + guard.
        assert!(
            advance
                .query
                .contains("op as var(func: eq(hodei.operation.id")
        );
        assert!(advance.query.contains("ops(func: eq(hodei.operation.id"));
        assert!(advance.query.contains("guard(func: eq(hodei.guard.key"));
        assert!(
            advance
                .query
                .contains("e as var(func: eq(hodei.identity.name")
        );
    }

    /// A fresh scope runs the CREATE branch: the guard is born at
    /// revision 1 with its first token, gated on the guard being absent
    /// inside the transaction.
    #[test]
    fn a_fresh_scope_creates_the_guard_with_the_first_token() {
        let effect = GuardedEffect::default();
        let create = guarded_upsert("tag|tenant-a|t@1", "op-1", 0, &effect);
        let block = &create.blocks[0];
        let cond = block.cond.as_deref().expect("the create block is gated");
        assert!(
            cond.contains("eq(len(g), 0)"),
            "the create block must be gated on the guard being absent: {cond}"
        );
        assert!(
            block.set_nquads.contains("_:guard <hodei.guard.key>"),
            "the create must write the conflict key"
        );
        assert!(
            block
                .set_nquads
                .contains("_:guard <hodei.guard.revision> \"1\""),
            "a fresh scope is born at revision 1"
        );
        assert!(
            block
                .set_nquads
                .contains("_:guard <hodei.guard.token> \"tag|tenant-a|t@1#1\""),
            "the create must write the first scope-qualified token"
        );
    }

    /// The in-transaction check decides whether the block applied: a
    /// guard still at the expected revision means it did; any other
    /// state means the pre-read was overtaken and the block committed
    /// nothing.
    #[test]
    fn the_in_transaction_check_detects_a_skipped_block() {
        let key = "tag|tenant-a|t@1";
        // Expected 0: applied only while the guard is absent.
        let fresh_absent = serde_json::json!({ "guard": [] });
        let fresh_present = serde_json::json!({
            "guard": [ { "hodei.guard.revision": 1 } ]
        });
        assert!(guard_block_applied(key, &fresh_absent, 0).expect("parses"));
        assert!(!guard_block_applied(key, &fresh_present, 0).expect("parses"));
        // Expected 4: applied only when the in-transaction revision is
        // still 4.
        let at_expected = serde_json::json!({
            "guard": [ { "hodei.guard.revision": 4 } ]
        });
        let moved_on = serde_json::json!({
            "guard": [ { "hodei.guard.revision": 7 } ]
        });
        let vanished = serde_json::json!({ "guard": [] });
        assert!(guard_block_applied(key, &at_expected, 4).expect("parses"));
        assert!(
            !guard_block_applied(key, &moved_on, 4).expect("parses"),
            "a pre-read overtaken by a concurrent commit must NOT report success"
        );
        assert!(!guard_block_applied(key, &vanished, 4).expect("parses"));
        // Two guard nodes are corruption, never a decision input.
        let duplicated = serde_json::json!({
            "guard": [
                { "hodei.guard.revision": 4 },
                { "hodei.guard.revision": 4 },
            ]
        });
        let error =
            guard_block_applied(key, &duplicated, 4).expect_err("duplicates are corruption");
        assert!(
            matches!(error, GuardError::CorruptState { .. }),
            "unexpected: {error}"
        );
    }

    /// The outcome of a replay carries the committed answer and no new
    /// effect; the outcome of a fresh commit carries the advanced
    /// revision. Pure shape check of the reported values.
    #[test]
    fn the_outcome_distinguishes_replay_from_commit() {
        let committed = GuardedOutcome {
            scope: "tag|tenant-a|t@1".to_owned(),
            operation_id: "op-1".to_owned(),
            guard_revision: 1,
            replayed: false,
            recorded_result: "ok".to_owned(),
        };
        let replayed = GuardedOutcome {
            replayed: true,
            guard_revision: 1,
            ..committed.clone()
        };
        assert!(!committed.replayed);
        assert!(replayed.replayed);
        assert_eq!(
            replayed.recorded_result, committed.recorded_result,
            "the replay returns the committed answer"
        );
        assert_eq!(
            replayed.guard_revision, committed.guard_revision,
            "a replay does not move the guard"
        );
    }
}
