//! The durable ledger of the account quota, over the graph authority.
//!
//! The in-memory ledger of [`crate::quota`] is a rate-shaping device: one
//! lock makes the check and the reservation the same step, and a restart
//! forgets it on purpose. This module keeps the SAME semantics -- the same
//! refusal rules, the same admission-then-pin flow, the same release on
//! abort -- but the state lives in the graph authority, so the decision
//! survives a process and is arbitrated by the server, not by one lock.
//!
//! The arbitration is the G06 guard mechanism: every account's quota is
//! ONE scope (`GuardKind::Quota`, tenant, account) derived with the same
//! conflict-key derivation every writer shares, every mutation of the
//! scope advances the scope's ONE revision inside its own transaction, and
//! the advance is anchored on a create-if-absent keyed revision marker
//! that the `@upsert` index makes unique. Two writers that hold the same
//! observed revision race on the SAME marker: the server aborts the loser,
//! the loser re-reads, re-decides with the same rules and reports the
//! refusal it now deserves. Writing the counters as absolutes computed
//! from an observed revision is only safe because the transaction
//! revalidates that revision inside its own snapshot; and the verdict of
//! every operation is read from the state AFTER the commit, never inferred
//! from the response's snapshot.
//!
//! The lifecycle is the crash-receipt pattern: a reservation is a keyed
//! ticket with a lease; the consumption settles it and writes its receipt
//! in ONE transaction (the receipt is the idempotency of the consumption,
//! so a lost ACK replays instead of counting twice); a ticket whose lease
//! died is released by the reconciler, which returns exactly what the dead
//! worker reserved, and an explicit release is the abort path.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphClientError, GuardKind, GuardScope, GuardStore, Tenant,
};
use serde_json::Value;

use crate::quota::{QuotaLimits, QuotaRefusal};

/// How many attempts a quota operation may lose before giving up. Every
/// attempt re-reads and re-decides, so a burst of contenders resolves one
/// serialized round each; the bound sheds load without risking a wrong
/// admission.
const MAX_ATTEMPTS: usize = 8;

/// Base delay of the conflict backoff; it grows per conflict round.
const RETRY_BASE: Duration = Duration::from_millis(10);

/// The backoff ceiling: under a wide race the exponential schedule would
/// outgrow the caller's patience.
const RETRY_CAP: Duration = Duration::from_millis(100);

/// The longest ticket or receipt identifier accepted. The identifiers
/// become parts of graph keys, so they obey the same charset a scope
/// subject does.
const MAX_ID_LEN: usize = 256;

/// The node type of one account's quota ledger.
pub const LEDGER_TYPE: &str = "HodeiQuotaLedger";
/// The node type of one upload's reservation.
pub const TICKET_TYPE: &str = "HodeiQuotaTicket";
/// The node type of one consumption's receipt.
pub const RECEIPT_TYPE: &str = "HodeiQuotaReceipt";
/// The node type of one revision's conflict anchor.
pub const TOKEN_TYPE: &str = "HodeiQuotaToken";

/// The DDL slice of this adapter: additive predicates and types, applied
/// idempotently after the versioned schema boot. It never touches a
/// predicate another slice owns.
pub const QUOTA_SCHEMA_DQL: &str = "
hodei.quota.key: string @index(exact) @upsert .
hodei.quota.tenant: string @index(exact) .
hodei.quota.account: string @index(exact) .
hodei.quota.stored: int .
hodei.quota.in_flight: int .
hodei.quota.active: int .
hodei.quota.revision: int @index(int) .
hodei.quota.updated_at: datetime .
hodei.quota.ticket.key: string @index(exact) @upsert .
hodei.quota.ticket.tenant: string @index(exact) .
hodei.quota.ticket.account: string @index(exact) .
hodei.quota.ticket.bytes: int .
hodei.quota.ticket.state: string @index(exact) .
hodei.quota.ticket.owner: string .
hodei.quota.ticket.lease_until: datetime .
hodei.quota.receipt.key: string @index(exact) @upsert .
hodei.quota.receipt.tenant: string @index(exact) .
hodei.quota.receipt.account: string @index(exact) .
hodei.quota.receipt.ticket: string @index(exact) .
hodei.quota.receipt.bytes: int .
hodei.quota.receipt.at: datetime @index(year) .
hodei.quota.token.key: string @index(exact) @upsert .
type HodeiQuotaLedger {
  hodei.quota.key
  hodei.quota.tenant
  hodei.quota.account
  hodei.quota.stored
  hodei.quota.in_flight
  hodei.quota.active
  hodei.quota.revision
  hodei.quota.updated_at
}
type HodeiQuotaTicket {
  hodei.quota.ticket.key
  hodei.quota.ticket.tenant
  hodei.quota.ticket.account
  hodei.quota.ticket.bytes
  hodei.quota.ticket.state
  hodei.quota.ticket.owner
  hodei.quota.ticket.lease_until
}
type HodeiQuotaReceipt {
  hodei.quota.receipt.key
  hodei.quota.receipt.tenant
  hodei.quota.receipt.account
  hodei.quota.receipt.ticket
  hodei.quota.receipt.bytes
  hodei.quota.receipt.at
}
type HodeiQuotaToken {
  hodei.quota.token.key
}
";

/// Why a quota admission over the graph failed. The refusals are the SAME
/// typed refusals the in-memory ledger answers with; the rest are the
/// graph's closed failures.
#[derive(Debug, thiserror::Error)]
pub enum QuotaFailure {
    /// The admission was refused, with the same meaning the in-memory
    /// ledger gives it (`413` or `429`).
    #[error(transparent)]
    Refusal(#[from] QuotaRefusal),
    /// The graph refused or could not answer.
    #[error(transparent)]
    Graph(#[from] QuotaGraphError),
}

/// Why a graph operation of the quota failed. Every variant is closed:
/// the caller knows exactly what happened and nothing was guessed.
#[derive(Debug, thiserror::Error)]
pub enum QuotaGraphError {
    /// The authority did not answer, or refused the transaction.
    #[error("the quota graph is not available: {0}")]
    Unavailable(String),
    /// The scope or an identifier is not a safe conflict key.
    #[error("the quota key '{key}' is not valid: {reason}")]
    InvalidKey {
        /// The key or identifier as received.
        key: String,
        /// Why it was rejected.
        reason: String,
    },
    /// The ticket the operation names does not exist.
    #[error("the ticket '{ticket}' does not exist in account '{account}'")]
    NoSuchTicket {
        /// The ticket id as received.
        ticket: String,
        /// The account it was asked for.
        account: String,
    },
    /// The ticket exists but is not reserved any more, so the operation
    /// that needs a reservation cannot run.
    #[error("the ticket '{ticket}' is '{state}', not reserved")]
    NotReserved {
        /// The ticket id.
        ticket: String,
        /// The state the ticket actually has.
        state: String,
    },
    /// The ticket's lease died: the worker that held it was declared
    /// dead, and a dead worker cannot pin or consume. The reconciler
    /// releases what it left.
    #[error("the lease of ticket '{ticket}' is dead, so nobody may pin or consume it")]
    LeaseDead {
        /// The ticket id whose lease died.
        ticket: String,
    },
    /// The write lost every attempt. Bounded, explicit, never silent.
    #[error(
        "the quota write on '{account}' did not commit after {attempts} attempts; \
         last failure: {last}"
    )]
    RetryExhausted {
        /// The account whose ledger was being written.
        account: String,
        /// How many attempts were made.
        attempts: usize,
        /// The last underlying failure.
        last: String,
    },
    /// The graph contradicts the ledger contract (duplicate rows for one
    /// key, a row without its counters).
    #[error("the quota state contradicts the ledger contract: {0}")]
    Corrupt(String),
}

impl QuotaGraphError {
    fn unavailable(error: impl std::fmt::Display) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// The ledger of one account, as an observation found it. An account that
/// has never been written reads as zeros at revision 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaObserved {
    /// The bytes the account has settled into the store.
    pub stored: u64,
    /// The bytes live reservations hold.
    pub in_flight: u64,
    /// The uploads with a live reservation.
    pub active: u64,
    /// The revision of the scope: every mutation advances it by one, and
    /// a write is only valid against the revision its basis observed.
    pub revision: u64,
}

impl QuotaObserved {
    /// The zeros of an account nobody has written yet.
    pub fn fresh() -> Self {
        Self {
            stored: 0,
            in_flight: 0,
            active: 0,
            revision: 0,
        }
    }
}

/// What a reservation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    /// The bytes the reservation holds (the declared size, or zero for a
    /// chunked upload that pins later).
    pub reserved: u64,
    /// The revision of the scope the reservation stands at.
    pub revision: u64,
    /// True when the ticket already stood: the same reservation re-driven
    /// found what a previous attempt committed, and nothing was counted
    /// twice.
    pub replayed: bool,
}

/// What a consumption did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    /// The account's stored total after the consumption.
    pub stored: u64,
    /// The receipt id the consumption answered to.
    pub receipt: String,
    /// True when the receipt already stood: the re-delivery of the same
    /// consumption counted nothing again.
    pub replayed: bool,
}

/// The admission decision of a reservation, pure: the SAME rules the
/// in-memory ledger applies, said separately so a caller can hold a
/// decision against the revision it observed (and so a test can hold two
/// of them behind a deterministic barrier). Returns the bytes the
/// reservation would hold.
pub fn decide_reserve(
    observed: &QuotaObserved,
    limits: &QuotaLimits,
    declared: Option<u64>,
) -> Result<u64, QuotaRefusal> {
    if let Some(max) = limits.max_concurrent_uploads_per_account
        && observed.active >= max as u64
    {
        return Err(QuotaRefusal::UploadsSaturated {
            retry_after_secs: limits.retry_after_secs,
        });
    }
    if let Some(size) = declared
        && let Some(max) = limits.max_stored_bytes_per_account
        && observed
            .stored
            .saturating_add(observed.in_flight)
            .saturating_add(size)
            > max
    {
        return Err(QuotaRefusal::StorageOver {
            retry_after_secs: limits.retry_after_secs,
        });
    }
    Ok(declared.unwrap_or(0))
}

/// The admission decision of a chunk pin, pure: the same rules
/// [`crate::quota::QuotaTicket::admit`] applies. Returns the bytes the pin
/// would ADD to the ledger.
fn decide_pin(
    observed: &QuotaObserved,
    limits: &QuotaLimits,
    ticket_bytes: u64,
    total_so_far: u64,
) -> Result<u64, QuotaRefusal> {
    let more = total_so_far.saturating_sub(ticket_bytes);
    if more == 0 {
        return Ok(0);
    }
    if let Some(max) = limits.max_stored_bytes_per_account
        && observed
            .stored
            .saturating_add(observed.in_flight)
            .saturating_add(more)
            > max
    {
        return Err(QuotaRefusal::StorageOver {
            retry_after_secs: limits.retry_after_secs,
        });
    }
    Ok(more)
}

/// One ticket as a read found it.
#[derive(Debug, Clone)]
struct TicketRead {
    bytes: u64,
    state: String,
    lease_until: Option<DateTime<Utc>>,
}

/// One scope read: the ledger as the graph holds it now, plus the rows the
/// operation names. The ledger here is never the decision basis of a
/// barrier-driven write: the caller's observed revision is.
#[derive(Debug, Clone)]
struct ScopeRead {
    ledger: QuotaObserved,
    ticket: Option<TicketRead>,
    receipt_present: bool,
}

/// The persistent quota ledger over the graph authority. Open through
/// [`DgraphQuotaStore::open`], which runs the versioned schema boot first
/// and then this adapter's additive DDL slice.
#[derive(Debug)]
pub struct DgraphQuotaStore {
    client: Arc<GraphClient>,
    guards: GuardStore,
}

impl DgraphQuotaStore {
    /// Opens the store on a connected client. The readiness boundary is
    /// the guard store's: an incompatible server is refused before any
    /// quota write path exists.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, QuotaGraphError> {
        let guards = GuardStore::open(client.clone())
            .await
            .map_err(QuotaGraphError::unavailable)?;
        apply_quota_schema(&client).await?;
        Ok(Self { client, guards })
    }

    /// The schema boot report this store validated at open.
    pub fn boot_report(&self) -> &hodei_graph_client::schema::SchemaBootReport {
        self.guards.boot_report()
    }

    /// The conflict scope of one account's quota: the G06 derivation every
    /// writer of the scope shares, so every code path that arbitrates the
    /// same account collides on the same conflict key.
    pub fn scope_of(tenant: &Tenant, account: &str) -> Result<GuardScope, QuotaGraphError> {
        GuardScope::new(GuardKind::Quota, tenant, account).map_err(|error| {
            QuotaGraphError::InvalidKey {
                key: error.to_string(),
                reason: "the account must be a safe scope subject".to_owned(),
            }
        })
    }

    /// The ledger key of a scope: the composite conflict key.
    fn ledger_key(scope: &GuardScope) -> String {
        hodei_graph_client::guard_key(scope)
    }

    /// The graph key of one ticket.
    fn ticket_key(ledger_key: &str, ticket_id: &str) -> String {
        format!("{ledger_key}/tickets/{ticket_id}")
    }

    /// The graph key of one receipt.
    fn receipt_key(ledger_key: &str, receipt_id: &str) -> String {
        format!("{ledger_key}/receipts/{receipt_id}")
    }

    /// The graph key of the conflict anchor of one revision.
    fn token_key(ledger_key: &str, revision: u64) -> String {
        format!("{ledger_key}#{revision}")
    }

    /// Validates a ticket or receipt identifier: it becomes part of a
    /// graph key, so it obeys the scope subject's charset.
    fn validate_id(kind: &str, id: &str) -> Result<(), QuotaGraphError> {
        let valid = !id.is_empty()
            && id.len() <= MAX_ID_LEN
            && id.starts_with(|c: char| c.is_ascii_alphanumeric())
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '@'));
        if valid {
            Ok(())
        } else {
            Err(QuotaGraphError::InvalidKey {
                key: id.to_owned(),
                reason: format!(
                    "the {kind} must be non-empty alphanumeric with '-', '_', '.', ':' and '@'"
                ),
            })
        }
    }

    /// Reads one scope: the ledger, the ticket (when asked) and whether
    /// the receipt already stands, in one round trip.
    async fn read_scope(
        &self,
        ledger_key: &str,
        ticket_graph_key: Option<&str>,
        receipt_graph_key: Option<&str>,
    ) -> Result<ScopeRead, QuotaGraphError> {
        let mut query = format!(
            "{{\n  led(func: eq(hodei.quota.key, {key})) {{ \
             hodei.quota.stored hodei.quota.in_flight hodei.quota.active hodei.quota.revision }}\n",
            key = dql_string(ledger_key),
        );
        if let Some(ticket) = ticket_graph_key {
            query.push_str(&format!(
                "  tick(func: eq(hodei.quota.ticket.key, {ticket})) {{ \
                 hodei.quota.ticket.bytes hodei.quota.ticket.state \
                 hodei.quota.ticket.lease_until }}\n",
                ticket = dql_string(ticket),
            ));
        }
        if let Some(receipt) = receipt_graph_key {
            query.push_str(&format!(
                "  rec(func: eq(hodei.quota.receipt.key, {receipt})) {{ uid }}\n",
                receipt = dql_string(receipt),
            ));
        }
        query.push('}');
        let response = query_json(&self.client, &query).await?;
        let ledgers = response
            .get("led")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let ledger = match ledgers.len() {
            0 => QuotaObserved::fresh(),
            1 => {
                let node = &ledgers[0];
                let stored = counter_of(node, "hodei.quota.stored").ok_or_else(|| {
                    QuotaGraphError::Corrupt("a ledger node without hodei.quota.stored".to_owned())
                })?;
                let in_flight = counter_of(node, "hodei.quota.in_flight").ok_or_else(|| {
                    QuotaGraphError::Corrupt(
                        "a ledger node without hodei.quota.in_flight".to_owned(),
                    )
                })?;
                let active = counter_of(node, "hodei.quota.active").ok_or_else(|| {
                    QuotaGraphError::Corrupt("a ledger node without hodei.quota.active".to_owned())
                })?;
                let revision = counter_of(node, "hodei.quota.revision").ok_or_else(|| {
                    QuotaGraphError::Corrupt(
                        "a ledger node without hodei.quota.revision".to_owned(),
                    )
                })?;
                QuotaObserved {
                    stored,
                    in_flight,
                    active,
                    revision,
                }
            }
            n => {
                return Err(QuotaGraphError::Corrupt(format!(
                    "{n} ledger nodes carry the key '{ledger_key}'"
                )));
            }
        };
        let ticket = match ticket_graph_key {
            None => None,
            Some(key) => {
                let nodes = response
                    .get("tick")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                match nodes.len() {
                    0 => None,
                    1 => {
                        let node = &nodes[0];
                        Some(TicketRead {
                            bytes: counter_of(node, "hodei.quota.ticket.bytes").ok_or_else(
                                || {
                                    QuotaGraphError::Corrupt(format!(
                                        "the ticket '{key}' has no bytes"
                                    ))
                                },
                            )?,
                            state: text_of(node, "hodei.quota.ticket.state").ok_or_else(|| {
                                QuotaGraphError::Corrupt(format!("the ticket '{key}' has no state"))
                            })?,
                            lease_until: node
                                .get("hodei.quota.ticket.lease_until")
                                .and_then(Value::as_str)
                                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                                .map(|instant| instant.with_timezone(&Utc)),
                        })
                    }
                    n => {
                        return Err(QuotaGraphError::Corrupt(format!(
                            "{n} ticket nodes carry the key '{key}'"
                        )));
                    }
                }
            }
        };
        let receipt_present = match receipt_graph_key {
            None => false,
            Some(_) => response
                .get("rec")
                .and_then(Value::as_array)
                .map(|nodes| !nodes.is_empty())
                .unwrap_or(false),
        };
        Ok(ScopeRead {
            ledger,
            ticket,
            receipt_present,
        })
    }

    /// What one account's ledger says right now. An account nobody wrote
    /// reads as zeros at revision 0; two ledger rows for one key are
    /// corruption, never a merged view.
    pub async fn observe(
        &self,
        tenant: &Tenant,
        account: &str,
    ) -> Result<QuotaObserved, QuotaGraphError> {
        let scope = Self::scope_of(tenant, account)?;
        let read = self
            .read_scope(&Self::ledger_key(&scope), None, None)
            .await?;
        Ok(read.ledger)
    }

    /// Admits an upload or refuses it, and persists the reservation it
    /// declared: the SAME admission the in-memory ledger makes, decided
    /// against the observed revision and arbitrated by the guard.
    ///
    /// `observed` is the basis of the decision: what the caller read (or
    /// was handed) as the state of the scope. The first attempt gates on
    /// THAT revision, so two writers that hold the same basis race for
    /// real on the SAME revision marker and the server decides. A loser
    /// re-reads, re-decides with the same rules against the state that
    /// committed meanwhile, and reports the refusal it now deserves; the
    /// same ticket id re-driven finds the standing reservation and replays
    /// it instead of reserving again.
    #[allow(clippy::too_many_arguments)]
    pub async fn reserve(
        &self,
        tenant: &Tenant,
        account: &str,
        ticket_id: &str,
        observed: &QuotaObserved,
        declared: Option<u64>,
        limits: &QuotaLimits,
        lease_owner: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<Reserved, QuotaFailure> {
        Self::validate_id("ticket", ticket_id)?;
        let scope = Self::scope_of(tenant, account)?;
        let ledger_key = Self::ledger_key(&scope);
        let ticket_graph_key = Self::ticket_key(&ledger_key, ticket_id);
        for attempt in 1..=MAX_ATTEMPTS {
            let read = self
                .read_scope(&ledger_key, Some(&ticket_graph_key), None)
                .await?;
            if let Some(ticket) = &read.ticket {
                // The reservation already stood. On the first look it is a
                // re-drive of a committed reservation; after a raced
                // attempt it is the twin that won the marker.
                return Ok(Reserved {
                    reserved: ticket.bytes,
                    revision: read.ledger.revision,
                    replayed: attempt == 1,
                });
            }
            // The decision basis: the caller's observation on the first
            // attempt (the barrier's handle), a fresh read after every
            // overtaken attempt. A refusal writes nothing.
            let basis = if attempt == 1 { observed } else { &read.ledger };
            let reserved = decide_reserve(basis, limits, declared)?;
            self.advance_scope(
                &ledger_key,
                basis,
                &read,
                &ticket_graph_key,
                None,
                ScopeAdvance::Reserve {
                    tenant: tenant.as_str(),
                    account,
                    declared: reserved,
                    lease_owner,
                    lease_until,
                },
            )
            .await?;
            // The verdict is the state AFTER the commit: the ticket stands
            // only if the transaction that created it committed.
            let after = self
                .read_scope(&ledger_key, Some(&ticket_graph_key), None)
                .await?;
            if let Some(ticket) = after.ticket {
                return Ok(Reserved {
                    reserved: ticket.bytes,
                    revision: after.ledger.revision,
                    replayed: false,
                });
            }
        }
        Err(QuotaGraphError::RetryExhausted {
            account: account.to_owned(),
            attempts: MAX_ATTEMPTS,
            last: "the reservation never stood".to_owned(),
        }
        .into())
    }

    /// Pins a streamed chunk onto a live reservation: the same admission
    /// [`crate::quota::QuotaTicket::admit`] makes, re-decided against the
    /// ledger at commit time. The pin renews the ticket's lease, because a
    /// body that is still streaming is a worker that is still alive.
    #[allow(clippy::too_many_arguments)]
    pub async fn pin(
        &self,
        tenant: &Tenant,
        account: &str,
        ticket_id: &str,
        total_so_far: u64,
        limits: &QuotaLimits,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), QuotaFailure> {
        Self::validate_id("ticket", ticket_id)?;
        let scope = Self::scope_of(tenant, account)?;
        let ledger_key = Self::ledger_key(&scope);
        let ticket_graph_key = Self::ticket_key(&ledger_key, ticket_id);
        let read = self
            .read_scope(&ledger_key, Some(&ticket_graph_key), None)
            .await?;
        let Some(ticket) = &read.ticket else {
            return Err(QuotaGraphError::NoSuchTicket {
                ticket: ticket_id.to_owned(),
                account: account.to_owned(),
            }
            .into());
        };
        if ticket.state != "reserved" {
            // A pin re-drive that finds its own bytes already pinned is
            // a constata; a pin that wants NEW bytes on a settled or
            // released ticket has nothing to pin onto.
            if ticket.state == "settled" && total_so_far <= ticket.bytes {
                return Ok(());
            }
            return Err(QuotaGraphError::NotReserved {
                ticket: ticket_id.to_owned(),
                state: ticket.state.clone(),
            }
            .into());
        }
        if ticket.lease_until.is_none_or(|until| until <= now) {
            return Err(QuotaGraphError::LeaseDead {
                ticket: ticket_id.to_owned(),
            }
            .into());
        }
        let more = decide_pin(&read.ledger, limits, ticket.bytes, total_so_far)?;
        if more == 0 {
            return Ok(());
        }
        // The pass is single: a lost revision CAS is retried inside
        // `advance_scope`, whose upsert gate revalidates the read state in
        // the commit snapshot. Re-running the decision here would re-read
        // a state the gate already refused.
        self.advance_scope(
            &ledger_key,
            &read.ledger,
            &read,
            &ticket_graph_key,
            None,
            ScopeAdvance::Pin {
                total_so_far,
                more,
                lease_until,
            },
        )
        .await?;
        Ok(())
    }

    /// Settles a reservation into stored bytes and writes the receipt of
    /// the consumption in the SAME transaction: the receipt cannot exist
    /// without the counters it explains, and the counters cannot move
    /// without their receipt. The receipt is the idempotency of the
    /// consumption: the re-delivery of the same `receipt_id` replays the
    /// committed answer and counts nothing again, whoever re-delivered it
    /// and however late.
    pub async fn consume(
        &self,
        tenant: &Tenant,
        account: &str,
        ticket_id: &str,
        stored_new_bytes: u64,
        receipt_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Consumed, QuotaGraphError> {
        Self::validate_id("ticket", ticket_id)?;
        Self::validate_id("receipt", receipt_id)?;
        let scope = Self::scope_of(tenant, account)?;
        let ledger_key = Self::ledger_key(&scope);
        let ticket_graph_key = Self::ticket_key(&ledger_key, ticket_id);
        let receipt_graph_key = Self::receipt_key(&ledger_key, receipt_id);
        for attempt in 1..=MAX_ATTEMPTS {
            let read = self
                .read_scope(
                    &ledger_key,
                    Some(&ticket_graph_key),
                    Some(&receipt_graph_key),
                )
                .await?;
            if read.receipt_present {
                return Ok(Consumed {
                    stored: read.ledger.stored,
                    receipt: receipt_id.to_owned(),
                    replayed: attempt == 1,
                });
            }
            let Some(ticket) = &read.ticket else {
                return Err(QuotaGraphError::NoSuchTicket {
                    ticket: ticket_id.to_owned(),
                    account: account.to_owned(),
                });
            };
            if ticket.state != "reserved" {
                return Err(QuotaGraphError::NotReserved {
                    ticket: ticket_id.to_owned(),
                    state: ticket.state.clone(),
                });
            }
            if ticket.lease_until.is_none_or(|until| until <= now) {
                return Err(QuotaGraphError::LeaseDead {
                    ticket: ticket_id.to_owned(),
                });
            }
            let stored_after = read.ledger.stored.saturating_add(stored_new_bytes);
            self.advance_scope(
                &ledger_key,
                &read.ledger,
                &read,
                &ticket_graph_key,
                Some(&receipt_graph_key),
                ScopeAdvance::Consume {
                    tenant: tenant.as_str(),
                    account,
                    stored_new_bytes,
                    stored_after,
                },
            )
            .await?;
            // The verdict is the state AFTER the commit: the receipt
            // stands only if the transaction that wrote it committed.
            let after = self
                .read_scope(
                    &ledger_key,
                    Some(&ticket_graph_key),
                    Some(&receipt_graph_key),
                )
                .await?;
            if after.receipt_present {
                return Ok(Consumed {
                    stored: after.ledger.stored,
                    receipt: receipt_id.to_owned(),
                    replayed: false,
                });
            }
        }
        Err(QuotaGraphError::RetryExhausted {
            account: account.to_owned(),
            attempts: MAX_ATTEMPTS,
            last: "the consumption never committed".to_owned(),
        })
    }

    /// Releases a reservation explicitly: the abort path, which returns
    /// exactly what the ticket held. It is a constata for a ticket that
    /// already settled or was already released, and it counts nothing
    /// twice however many times it is driven.
    pub async fn release(
        &self,
        tenant: &Tenant,
        account: &str,
        ticket_id: &str,
    ) -> Result<bool, QuotaGraphError> {
        Self::validate_id("ticket", ticket_id)?;
        let scope = Self::scope_of(tenant, account)?;
        let ledger_key = Self::ledger_key(&scope);
        let ticket_graph_key = Self::ticket_key(&ledger_key, ticket_id);
        let read = self
            .read_scope(&ledger_key, Some(&ticket_graph_key), None)
            .await?;
        let Some(ticket) = &read.ticket else {
            return Ok(false);
        };
        if ticket.state != "reserved" {
            return Ok(false);
        }
        // The pass is single: a lost revision CAS is retried inside
        // `advance_scope` against the same gate, and the release is a
        // constata that advances nothing when the ticket no longer holds.
        self.advance_scope(
            &ledger_key,
            &read.ledger,
            &read,
            &ticket_graph_key,
            None,
            ScopeAdvance::Release {
                ticket_bytes: ticket.bytes,
            },
        )
        .await?;
        Ok(true)
    }

    /// The reconciler: releases every reserved ticket whose lease died,
    /// returning what the dead workers left. A ticket that settled before
    /// its lease died is not touched -- its bytes are already stored, and
    /// releasing them would un-count real content.
    pub async fn release_expired(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, QuotaGraphError> {
        let query = format!(
            "{{ q(func: eq(hodei.quota.ticket.state, \"reserved\")) \
             @filter(le(hodei.quota.ticket.lease_until, {now})) \
             {{ hodei.quota.ticket.key hodei.quota.ticket.tenant hodei.quota.ticket.account }} }}",
            now = dql_string(&now.to_rfc3339()),
        );
        let response = query_json(&self.client, &query).await?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut released = Vec::new();
        for node in nodes {
            let key = text_of(&node, "hodei.quota.ticket.key").ok_or_else(|| {
                QuotaGraphError::Corrupt("an expired ticket has no key".to_owned())
            })?;
            let tenant = text_of(&node, "hodei.quota.ticket.tenant").ok_or_else(|| {
                QuotaGraphError::Corrupt(format!("the ticket '{key}' has no tenant"))
            })?;
            let account = text_of(&node, "hodei.quota.ticket.account").ok_or_else(|| {
                QuotaGraphError::Corrupt(format!("the ticket '{key}' has no account"))
            })?;
            let ticket_id = key.rsplit('/').next().unwrap_or(&key).to_owned();
            let tenant = Tenant::new(&tenant).map_err(|error| {
                QuotaGraphError::Corrupt(format!(
                    "the ticket '{key}' names an invalid tenant: {error}"
                ))
            })?;
            if self.release(&tenant, &account, &ticket_id).await? {
                released.push(key);
            }
        }
        Ok(released)
    }

    /// Advances the scope by one revision: ONE transaction that carries
    /// the ledger's new absolutes, the operation's own row, the NEXT
    /// revision's conflict anchor (created only when absent, which is what
    /// forces two concurrent writers to abort) and the removal of the
    /// previous anchor. The `basis` is the revision the absolutes were
    /// computed from; the gate revalidates THAT revision and the row's
    /// preconditions inside the transaction's own snapshot, so an
    /// overtaken basis writes nothing and costs a retry.
    async fn advance_scope(
        &self,
        ledger_key: &str,
        basis: &QuotaObserved,
        read: &ScopeRead,
        ticket_graph_key: &str,
        receipt_graph_key: Option<&str>,
        advance: ScopeAdvance<'_>,
    ) -> Result<(), QuotaGraphError> {
        let current = basis.revision;
        let next = current + 1;
        let next_token = Self::token_key(ledger_key, next);
        let prev_token = Self::token_key(ledger_key, current);
        let updated_at = dql_string(&Utc::now().to_rfc3339());

        // The query: the ledger revalidated at the basis revision, the
        // ticket (created if absent, or revalidated at its precondition),
        // the anchors and the receipt when the effect carries one.
        let mut query = if current == 0 {
            format!(
                "{{\n  led as var(func: eq(hodei.quota.key, {key}))\n",
                key = dql_string(ledger_key),
            )
        } else {
            format!(
                "{{\n  led as var(func: eq(hodei.quota.key, {key})) \
                 @filter(eq(hodei.quota.revision, {current}))\n",
                key = dql_string(ledger_key),
            )
        };
        query.push_str(&format!(
            "  tick as var(func: eq(hodei.quota.ticket.key, {ticket}))",
            ticket = dql_string(ticket_graph_key),
        ));
        query.push_str(match advance {
            ScopeAdvance::Reserve { .. } => "\n",
            _ => " @filter(eq(hodei.quota.ticket.state, \"reserved\"))\n",
        });
        query.push_str(&format!(
            "  tok as var(func: eq(hodei.quota.token.key, {next_token}))\n",
            next_token = dql_string(&next_token),
        ));
        if current > 0 {
            query.push_str(&format!(
                "  prev as var(func: eq(hodei.quota.token.key, {prev_token}))\n",
                prev_token = dql_string(&prev_token),
            ));
        }
        if let Some(receipt) = receipt_graph_key {
            query.push_str(&format!(
                "  rec as var(func: eq(hodei.quota.receipt.key, {receipt}))\n",
                receipt = dql_string(receipt),
            ));
        }
        query.push('}');

        // The gate conjuncts: the ledger at its branch precondition, the
        // anchor absent (create-if-absent forces the conflict), the ticket
        // on its branch precondition, the receipt absent when the effect
        // writes one.
        let ledger_gate = if current == 0 {
            "eq(len(led), 0)"
        } else {
            "eq(len(led), 1)"
        };
        let ticket_gate = match advance {
            ScopeAdvance::Reserve { .. } => "eq(len(tick), 0)",
            _ => "eq(len(tick), 1)",
        };
        let receipt_gate = if receipt_graph_key.is_some() {
            " AND eq(len(rec), 0)"
        } else {
            ""
        };
        let gate = format!("{ledger_gate} AND eq(len(tok), 0) AND {ticket_gate}{receipt_gate}");

        let nquads = match &advance {
            ScopeAdvance::Reserve {
                tenant,
                account,
                declared,
                lease_owner,
                lease_until,
            } => {
                let (in_flight, active) = if current == 0 {
                    (*declared, 1u64)
                } else {
                    (
                        basis.in_flight.saturating_add(*declared),
                        basis.active.saturating_add(1),
                    )
                };
                let mut quads = if current == 0 {
                    format!(
                        "_:led <dgraph.type> \"{led_type}\" .\n\
                         _:led <hodei.quota.key> {key} .\n\
                         _:led <hodei.quota.tenant> {tenant} .\n\
                         _:led <hodei.quota.account> {account} .\n\
                         _:led <hodei.quota.stored> \"{stored}\" .\n\
                         _:led <hodei.quota.in_flight> \"{in_flight}\" .\n\
                         _:led <hodei.quota.active> \"{active}\" .\n\
                         _:led <hodei.quota.revision> \"{revision}\" .\n\
                         _:led <hodei.quota.updated_at> {now} .\n",
                        led_type = LEDGER_TYPE,
                        key = dql_string(ledger_key),
                        tenant = dql_string(tenant),
                        account = dql_string(account),
                        stored = basis.stored,
                        in_flight = in_flight,
                        active = active,
                        revision = next,
                        now = updated_at,
                    )
                } else {
                    format!(
                        "uid(led) <hodei.quota.in_flight> \"{in_flight}\" .\n\
                         uid(led) <hodei.quota.active> \"{active}\" .\n\
                         uid(led) <hodei.quota.revision> \"{revision}\" .\n\
                         uid(led) <hodei.quota.updated_at> {now} .\n",
                        in_flight = in_flight,
                        active = active,
                        revision = next,
                        now = updated_at,
                    )
                };
                quads.push_str(&ticket_nquads(
                    ticket_graph_key,
                    tenant,
                    account,
                    *declared,
                    "reserved",
                    lease_owner,
                    lease_until,
                ));
                quads
            }
            ScopeAdvance::Pin {
                total_so_far,
                more,
                lease_until,
            } => {
                let in_flight = basis.in_flight.saturating_add(*more);
                let mut quads = format!(
                    "uid(led) <hodei.quota.in_flight> \"{in_flight}\" .\n\
                     uid(led) <hodei.quota.revision> \"{revision}\" .\n\
                     uid(led) <hodei.quota.updated_at> {now} .\n",
                    in_flight = in_flight,
                    revision = next,
                    now = updated_at,
                );
                quads.push_str(&format!(
                    "uid(tick) <hodei.quota.ticket.bytes> \"{total}\" .\n\
                     uid(tick) <hodei.quota.ticket.lease_until> {until} .\n",
                    total = total_so_far,
                    until = dql_string(&lease_until.to_rfc3339()),
                ));
                quads
            }
            ScopeAdvance::Consume {
                tenant,
                account,
                stored_new_bytes,
                stored_after,
            } => {
                let ticket_bytes = read.ticket.as_ref().map(|t| t.bytes).unwrap_or(0);
                let in_flight = basis.in_flight.saturating_sub(ticket_bytes);
                let active = basis.active.saturating_sub(1);
                let mut quads = format!(
                    "uid(led) <hodei.quota.stored> \"{stored}\" .\n\
                     uid(led) <hodei.quota.in_flight> \"{in_flight}\" .\n\
                     uid(led) <hodei.quota.active> \"{active}\" .\n\
                     uid(led) <hodei.quota.revision> \"{revision}\" .\n\
                     uid(led) <hodei.quota.updated_at> {now} .\n",
                    stored = stored_after,
                    in_flight = in_flight,
                    active = active,
                    revision = next,
                    now = updated_at,
                );
                quads.push_str("uid(tick) <hodei.quota.ticket.state> \"settled\" .\n");
                quads.push_str(&receipt_nquads(
                    receipt_graph_key.expect("the consume carries a receipt"),
                    tenant,
                    account,
                    ticket_graph_key,
                    *stored_new_bytes,
                ));
                quads
            }
            ScopeAdvance::Release { ticket_bytes } => {
                let in_flight = basis.in_flight.saturating_sub(*ticket_bytes);
                let active = basis.active.saturating_sub(1);
                format!(
                    "uid(led) <hodei.quota.in_flight> \"{in_flight}\" .\n\
                     uid(led) <hodei.quota.active> \"{active}\" .\n\
                     uid(led) <hodei.quota.revision> \"{revision}\" .\n\
                     uid(led) <hodei.quota.updated_at> {now} .\n\
                     uid(tick) <hodei.quota.ticket.state> \"released\" .\n",
                    in_flight = in_flight,
                    active = active,
                    revision = next,
                    now = updated_at,
                )
            }
        };
        // The anchor of the next revision, created only when absent: two
        // writers advancing from the same revision race on the SAME key,
        // and the `@upsert` index aborts the loser. The previous anchor
        // goes away with the advance, and only behind the same gate: what
        // the gate refused is never deleted.
        let anchor = format!(
            "_:tok <dgraph.type> \"{token_type}\" .\n\
             _:tok <hodei.quota.token.key> {next_token} .\n",
            token_type = TOKEN_TYPE,
            next_token = dql_string(&next_token),
        );
        let blocks = vec![GraphMutationBlock {
            cond: Some(gate),
            set_nquads: format!("{nquads}{anchor}"),
            delete_nquads: if current > 0 {
                "uid(prev) * * .\n".to_owned()
            } else {
                String::new()
            },
        }];
        let mut last = String::new();
        for conflict_round in 1..=MAX_ATTEMPTS {
            match self
                .client
                .upsert(GraphUpsertTransaction {
                    query: query.clone(),
                    blocks: blocks.clone(),
                    commit_now: true,
                })
                .await
            {
                Ok(response) => {
                    require_no_errors(&response.json)?;
                    return Ok(());
                }
                Err(GraphClientError::UpsertConflict { detail }) => {
                    last = detail;
                    let delay = (RETRY_BASE * conflict_round as u32).min(RETRY_CAP);
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(QuotaGraphError::unavailable(error)),
            }
        }
        Err(QuotaGraphError::RetryExhausted {
            account: ledger_key.to_owned(),
            attempts: MAX_ATTEMPTS,
            last,
        })
    }
}

/// What one advance of the scope does, said as data so the transaction
/// builder stays one function with one gate shape.
#[derive(Debug, Clone, Copy)]
enum ScopeAdvance<'a> {
    /// A new reservation: create the ticket, take its bytes in flight.
    Reserve {
        tenant: &'a str,
        account: &'a str,
        declared: u64,
        lease_owner: &'a str,
        lease_until: DateTime<Utc>,
    },
    /// A chunk pin: the ticket's bytes grow to the running total.
    Pin {
        total_so_far: u64,
        more: u64,
        lease_until: DateTime<Utc>,
    },
    /// A consumption: stored grows, the ticket settles, the receipt stands.
    Consume {
        tenant: &'a str,
        account: &'a str,
        stored_new_bytes: u64,
        stored_after: u64,
    },
    /// An abort: the ticket's reservation comes back.
    Release { ticket_bytes: u64 },
}

/// The N-Quads that write one ticket row.
fn ticket_nquads(
    key: &str,
    tenant: &str,
    account: &str,
    bytes: u64,
    state: &str,
    owner: &str,
    lease_until: &DateTime<Utc>,
) -> String {
    format!(
        "_:tick <dgraph.type> \"{ticket_type}\" .\n\
         _:tick <hodei.quota.ticket.key> {key} .\n\
         _:tick <hodei.quota.ticket.tenant> {tenant} .\n\
         _:tick <hodei.quota.ticket.account> {account} .\n\
         _:tick <hodei.quota.ticket.bytes> \"{bytes}\" .\n\
         _:tick <hodei.quota.ticket.state> \"{state}\" .\n\
         _:tick <hodei.quota.ticket.owner> {owner} .\n\
         _:tick <hodei.quota.ticket.lease_until> {until} .\n",
        ticket_type = TICKET_TYPE,
        key = dql_string(key),
        tenant = dql_string(tenant),
        account = dql_string(account),
        bytes = bytes,
        state = state,
        owner = dql_string(owner),
        until = dql_string(&lease_until.to_rfc3339()),
    )
}

/// The N-Quads that write one receipt row.
fn receipt_nquads(key: &str, tenant: &str, account: &str, ticket_key: &str, bytes: u64) -> String {
    format!(
        "_:rec <dgraph.type> \"{receipt_type}\" .\n\
         _:rec <hodei.quota.receipt.key> {key} .\n\
         _:rec <hodei.quota.receipt.tenant> {tenant} .\n\
         _:rec <hodei.quota.receipt.account> {account} .\n\
         _:rec <hodei.quota.receipt.ticket> {ticket} .\n\
         _:rec <hodei.quota.receipt.bytes> \"{bytes}\" .\n\
         _:rec <hodei.quota.receipt.at> {at} .\n",
        receipt_type = RECEIPT_TYPE,
        key = dql_string(key),
        tenant = dql_string(tenant),
        account = dql_string(account),
        ticket = dql_string(ticket_key),
        bytes = bytes,
        at = dql_string(&Utc::now().to_rfc3339()),
    )
}

/// The counter an int predicate names, when the node carries it.
fn counter_of(node: &Value, predicate: &str) -> Option<u64> {
    node.get(predicate).and_then(Value::as_u64)
}

/// The text a string predicate names, when the node carries it.
fn text_of(node: &Value, predicate: &str) -> Option<String> {
    node.get(predicate)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Runs one DQL query and answers its data, refusing the error arrays the
/// wire may carry.
async fn query_json(client: &GraphClient, query: &str) -> Result<Value, QuotaGraphError> {
    let response = client
        .execute_dql(query)
        .await
        .map_err(QuotaGraphError::unavailable)?;
    require_no_errors(&response.json)?;
    Ok(response.json)
}

/// A server-side refusal travels inside the JSON: it is never read as a
/// successful answer.
fn require_no_errors(json: &Value) -> Result<(), QuotaGraphError> {
    let empty: Vec<Value> = Vec::new();
    let errors = json
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if errors.is_empty() {
        return Ok(());
    }
    let text = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Err(QuotaGraphError::Unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

/// Applies the additive DDL slice of this adapter. Idempotent, bounded:
/// two stores opening at once alter the same additive slice, and Dgraph
/// refuses one of them while the indexes settle.
async fn apply_quota_schema(client: &GraphClient) -> Result<(), QuotaGraphError> {
    let mut last = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        match client.alter_schema(QUOTA_SCHEMA_DQL).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < MAX_ATTEMPTS => {
                last = error.to_string();
                tokio::time::sleep(RETRY_BASE * attempt as u32).await;
            }
            Err(error) => return Err(QuotaGraphError::unavailable(error)),
        }
    }
    Err(QuotaGraphError::Unavailable(format!(
        "the quota DDL slice did not settle after {MAX_ATTEMPTS} attempts: {last}"
    )))
}
