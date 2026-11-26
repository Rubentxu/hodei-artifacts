//! The trust grants and the promotion decisions over the graph authority.
//!
//! This adapter replaces the retired Surreal ones: the
//! deployment chooses one store, the domain chooses neither. What it adds
//! is the profile the trusted channel needs on the shared graph:
//!
//! - **Strict read-on-access, authoritative.** Every `active_grant` runs
//!   a fresh query against the graph authority. Dgraph queries default
//!   to linearizable reads (`best_effort` is never set): the answer is
//!   evaluated at the latest committed timestamp, so a revocation whose
//!   commit is visible cannot be answered by a stale snapshot. There is
//!   no TTL, no negative cache and no best-effort read: what the ledger
//!   answers is what the ledger has, and any failure of the authority is
//!   an error, never a permission.
//! - **The event travels with the change.** The grant or revocation and
//!   its audit event are one upsert transaction: if the row moved, the
//!   event moved with it. An authorization whose record did not commit
//!   did not commit at all (the outbox pattern of the stack).
//! - **Revocation is a compare-and-set.** The revocation only applies to
//!   the grants that are live INSIDE the committing transaction: a
//!   revoked grant is never touched twice, and the count a revocation
//!   returns is what it actually moved. A revocation whose commit is
//!   visible answers zero to every later revocation of the same grants.
//! - **Activation is a guarded CAS.** Publishing a new grant version for
//!   one digest aggregate goes through the graph guards with
//!   [`RevisionDemand::At`]: two writers activating different decisions
//!   on the same aggregate cannot both confirm -- one wins and the other
//!   receives the revision the aggregate actually reached, reads the
//!   aggregate and says who won. This is the HSP-149 activation guard
//!   (the Surreal `THROW` revalidating the head inside the transaction)
//!   ported to the graph guards.
//!
//! The schema of this adapter is its own DDL slice: additive, idempotent,
//! applied by [`ensure_trust_schema`] after the versioned schema boot.
//! The shared schema in `hodei_graph_client::schema` is not touched.
//!
//! The same adapter also answers for the reactive work: the intents of
//! [`hodei_reactive::intent::IntentOutbox`] and the execution records of
//! [`hodei_reactive::runtime::ExecutionStore`] live here as the third
//! port of the store, with the SAME mechanics the ledger uses.
//!
//! - **The claim is a compare-and-set.** The state transition is gated on
//!   the state the transaction itself reads, and the state and the lease
//!   generation are `@upsert` predicates: two workers claiming the same
//!   intent write the same value on the same node and the server aborts
//!   one of them. The retry re-reads: if the winner's lease is live the
//!   loser finds nothing claimable and says so.
//! - **The lease generation is the fencing token, and it is monotonic.**
//!   Every claim that lands writes `generation = read + 1` behind the
//!   in-transaction gate `generation == read`, so a generation can only
//!   grow. A worker whose lease died cannot write a receipt (the receipt
//!   is gated on a live lease), and a worker whose token was overtaken
//!   cannot write even through the fenced API: both refusals are typed
//!   errors, never silent no-ops.
//! - **The receipt and the effect are ONE fact.** An `Applied` receipt
//!   lands with the effect row — keyed by the effect ID, created only
//!   when absent — in the same transaction. The effect cannot exist
//!   without its receipt, and a re-claimer that finds either closes the
//!   intent without applying again.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, GrantSweepRow, GrantSweepSource, TrustGrant, TrustLedger};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphClientError, GuardError, GuardKind, GuardScope, GuardStore, GuardedEffect,
    OperationId, RevisionDemand, Tenant,
};
use hodei_reactive::behavior::Effect;
use hodei_reactive::execution::ExecutionRecord;
use hodei_reactive::intent::{Intent, IntentOutbox, Receipt};
use hodei_reactive::runtime::ExecutionStore;
use hodei_trust_policy::ports::{DecisionStore, DecisionStoreError};
use hodei_trust_policy::promotion::{DecisionId, PromotionDecision};
use serde_json::Value;

/// How many times a write that lost a server conflict may retry before
/// the failure is reported. The retry re-runs the whole upsert, so the
/// revalidation happens by construction.
const MAX_WRITE_ATTEMPTS: usize = 3;

/// Base delay of the retry backoff; it doubles per attempt.
const RETRY_BASE: Duration = Duration::from_millis(25);

/// The node type of a grant row.
pub const GRANT_TYPE: &str = "HodeiTrustGrant";
/// The node type of an audit event of the ledger.
pub const EVENT_TYPE: &str = "HodeiTrustGrantEvent";
/// The node type of a persisted promotion decision.
pub const DECISION_TYPE: &str = "HodeiPromotionDecision";

/// The DDL slice of this adapter: additive predicates and types, applied
/// idempotently after the versioned schema boot.
pub const TRUST_SCHEMA_DQL: &str = "
hodei.trust.grant.key: string @index(exact) @upsert .
hodei.trust.grant.account: string @index(exact) .
hodei.trust.grant.repository: string @index(exact) .
hodei.trust.grant.digest_algorithm: string @index(exact) .
hodei.trust.grant.digest: string @index(exact) .
hodei.trust.grant.decision_id: string @index(exact) .
hodei.trust.grant.actor: string .
hodei.trust.grant.granted_at: datetime @index(year) .
hodei.trust.grant.revoked_at: datetime @index(year) @upsert .
hodei.trust.grant.revoked_by: string .
hodei.trust.grant.revoke_reason: string .
hodei.trust.grant.subject: string @index(exact) .
hodei.trust.grant.policy_version: string .
hodei.trust.grant.capsule_hash: string .
hodei.trust.grant.risk_version: int .
hodei.trust.grant.valid_until: datetime .
hodei.trust.event.key: string @index(exact) @upsert .
hodei.trust.event.kind: string @index(exact) .
hodei.trust.event.account: string @index(exact) .
hodei.trust.event.repository: string @index(exact) .
hodei.trust.event.decision_id: string @index(exact) .
hodei.trust.event.digest: string @index(exact) .
hodei.trust.event.actor: string .
hodei.trust.event.reason: string .
hodei.trust.event.at: datetime @index(year) .
hodei.trust.event.payload: string .
hodei.trust.event.grant: [uid] @reverse .
hodei.trust.decision.key: string @index(exact) @upsert .
hodei.trust.decision.account: string @index(exact) .
hodei.trust.decision.subject: string @index(exact) .
hodei.trust.decision.correlation_id: string @index(exact) .
hodei.trust.decision.outcome: string @index(exact) .
hodei.trust.decision.distribution: string @index(exact) .
hodei.trust.decision.decided_at: datetime @index(year) .
hodei.trust.decision.payload: string .
type HodeiTrustGrant {
  hodei.trust.grant.key
  hodei.trust.grant.account
  hodei.trust.grant.repository
  hodei.trust.grant.digest_algorithm
  hodei.trust.grant.digest
  hodei.trust.grant.decision_id
  hodei.trust.grant.actor
  hodei.trust.grant.granted_at
  hodei.trust.grant.revoked_at
  hodei.trust.grant.revoked_by
  hodei.trust.grant.revoke_reason
  hodei.trust.grant.subject
  hodei.trust.grant.policy_version
  hodei.trust.grant.capsule_hash
  hodei.trust.grant.risk_version
  hodei.trust.grant.valid_until
}
type HodeiTrustGrantEvent {
  hodei.trust.event.key
  hodei.trust.event.kind
  hodei.trust.event.account
  hodei.trust.event.repository
  hodei.trust.event.decision_id
  hodei.trust.event.digest
  hodei.trust.event.actor
  hodei.trust.event.reason
  hodei.trust.event.at
  hodei.trust.event.payload
  hodei.trust.event.grant
}
type HodeiPromotionDecision {
  hodei.trust.decision.key
  hodei.trust.decision.account
  hodei.trust.decision.subject
  hodei.trust.decision.correlation_id
  hodei.trust.decision.outcome
  hodei.trust.decision.distribution
  hodei.trust.decision.decided_at
  hodei.trust.decision.payload
}
hodei.intent.key: string @index(exact) @upsert .
hodei.intent.tenant: string @index(exact) .
hodei.intent.state: string @index(exact) @upsert .
hodei.intent.kind: string @index(exact) .
hodei.intent.idempotency: string @index(exact) .
hodei.intent.lease_owner: string .
hodei.intent.lease_until: datetime .
hodei.intent.lease_generation: int @index(int) @upsert .
hodei.intent.receipt: string .
hodei.intent.payload: string .
hodei.effect.key: string @index(exact) @upsert .
hodei.effect.kind: string .
hodei.effect.payload: string .
hodei.effect.generation: int .
hodei.execution.key: string @index(exact) @upsert .
hodei.execution.behavior: string @index(exact) .
hodei.execution.event: string @index(exact) .
hodei.execution.started_at: datetime @index(year) .
hodei.execution.payload: string .
hodei.claim.key: string @index(exact) @upsert .
hodei.claim.identity: string @index(exact) .
hodei.claim.owner: string .
hodei.claim.at: datetime @index(year) .
hodei.claim.generation: int .
type HodeiReactiveIntent {
  hodei.intent.key
  hodei.intent.tenant
  hodei.intent.state
  hodei.intent.kind
  hodei.intent.idempotency
  hodei.intent.lease_owner
  hodei.intent.lease_until
  hodei.intent.lease_generation
  hodei.intent.receipt
  hodei.intent.payload
}
type HodeiReactiveEffect {
  hodei.effect.key
  hodei.effect.kind
  hodei.effect.payload
  hodei.effect.generation
}
type HodeiReactiveExecution {
  hodei.execution.key
  hodei.execution.behavior
  hodei.execution.event
  hodei.execution.started_at
  hodei.execution.payload
}
type HodeiIntentClaim {
  hodei.claim.key
  hodei.claim.identity
  hodei.claim.owner
  hodei.claim.at
  hodei.claim.generation
}
";

/// Why the graph could not answer for the trust.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DgraphTrustError {
    /// The authority did not answer, or refused the transaction: the
    /// caller must fail closed, never read a permission into it.
    #[error("the trust graph is not available: {0}")]
    Unavailable(String),
    /// The graph contradicts its own ledger contract (duplicate rows for
    /// one key, a row that does not parse): a real invariant break.
    #[error("the trust ledger state is corrupt: {0}")]
    Corrupt(String),
}

impl DgraphTrustError {
    fn unavailable(error: impl std::fmt::Display) -> Self {
        Self::Unavailable(error.to_string())
    }
}

impl From<GuardError> for DgraphTrustError {
    fn from(error: GuardError) -> Self {
        DgraphTrustError::Unavailable(error.to_string())
    }
}

impl From<DgraphTrustError> for CatalogError {
    fn from(error: DgraphTrustError) -> Self {
        match error {
            DgraphTrustError::Unavailable(detail) => CatalogError::unavailable(detail),
            DgraphTrustError::Corrupt(detail) => CatalogError::Corrupt {
                kind: "trust grant",
                key: "ledger".to_owned(),
                detail,
            },
        }
    }
}

impl From<DgraphTrustError> for DecisionStoreError {
    fn from(error: DgraphTrustError) -> Self {
        DecisionStoreError::Unavailable(error.to_string())
    }
}

/// The UTC instant now, RFC 3339 (what a Dgraph `datetime` accepts).
fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The instant a stored datetime names, or corruption when it does not
/// parse: a ledger that cannot read its own times cannot judge validity,
/// and it says so instead of guessing.
fn instant_of(text: &str) -> Result<DateTime<Utc>, DgraphTrustError> {
    DateTime::parse_from_rfc3339(text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|error| DgraphTrustError::Corrupt(format!("unparsable datetime: {error}")))
}

/// The digest a stored (algorithm, value) pair names, or corruption.
fn digest_of(algorithm: &str, value: &str) -> Result<ContentDigest, DgraphTrustError> {
    match algorithm {
        "sha256" => Ok(ContentDigest::sha256(value)),
        "sha512" => Ok(ContentDigest {
            algorithm: DigestAlgorithm::Sha512,
            value: value.to_ascii_lowercase(),
        }),
        other => Err(DgraphTrustError::Corrupt(format!(
            "unknown digest algorithm: {other}"
        ))),
    }
}

/// How many digests one `active_digests` read names at most: the query is
/// text, and a page of a listing is far below this, so the chunking only
/// bounds the size of what is sent for a caller that asks for many.
const ACTIVE_DIGESTS_CHUNK: usize = 100;

/// The grant row fields a read brings back.
const GRANT_FIELDS: &str = "
  uid
  hodei.trust.grant.key
  hodei.trust.grant.account
  hodei.trust.grant.repository
  hodei.trust.grant.digest_algorithm
  hodei.trust.grant.digest
  hodei.trust.grant.decision_id
  hodei.trust.grant.actor
  hodei.trust.grant.granted_at
  hodei.trust.grant.revoked_at
  hodei.trust.grant.revoked_by
  hodei.trust.grant.revoke_reason
  hodei.trust.grant.subject
  hodei.trust.grant.policy_version
  hodei.trust.grant.capsule_hash
  hodei.trust.grant.risk_version
  hodei.trust.grant.valid_until
";

/// One string field of a node, when the node carries it.
fn text_of(node: &Value, predicate: &str) -> Option<String> {
    node.get(predicate)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// One string field the node must carry: its absence is corruption.
fn required_text(node: &Value, predicate: &str) -> Result<String, DgraphTrustError> {
    text_of(node, predicate)
        .ok_or_else(|| DgraphTrustError::Corrupt(format!("a grant node without {predicate}")))
}

/// The grant a stored node names.
fn grant_of(node: &Value) -> Result<TrustGrant, DgraphTrustError> {
    let algorithm = required_text(node, "hodei.trust.grant.digest_algorithm")?;
    let value = required_text(node, "hodei.trust.grant.digest")?;
    let granted_at = instant_of(&required_text(node, "hodei.trust.grant.granted_at")?)?;
    let revoked_at = match text_of(node, "hodei.trust.grant.revoked_at") {
        Some(text) => Some(instant_of(&text)?),
        None => None,
    };
    let valid_until = match text_of(node, "hodei.trust.grant.valid_until") {
        Some(text) => Some(instant_of(&text)?),
        None => None,
    };
    Ok(TrustGrant {
        account: required_text(node, "hodei.trust.grant.account")?,
        repository: required_text(node, "hodei.trust.grant.repository")?,
        digest: digest_of(&algorithm, &value)?,
        decision_id: required_text(node, "hodei.trust.grant.decision_id")?,
        actor: required_text(node, "hodei.trust.grant.actor")?,
        granted_at,
        revoked_at,
        revoked_by: text_of(node, "hodei.trust.grant.revoked_by"),
        revoke_reason: text_of(node, "hodei.trust.grant.revoke_reason"),
        subject: text_of(node, "hodei.trust.grant.subject"),
        policy_version: text_of(node, "hodei.trust.grant.policy_version"),
        capsule_hash: text_of(node, "hodei.trust.grant.capsule_hash"),
        risk_version: node
            .get("hodei.trust.grant.risk_version")
            .and_then(Value::as_i64)
            .filter(|version| *version >= 0)
            .map(|version| version as u64),
        valid_until,
    })
}

/// The identity of a grant row: account, repository, digest and decision,
/// the same composite the Surreal row uses.
pub fn grant_key(grant: &TrustGrant) -> String {
    format!(
        "{account}|{repository}|{algorithm}|{value}|{decision}",
        account = grant.account,
        repository = grant.repository,
        algorithm = grant.digest.algorithm,
        value = grant.digest.value,
        decision = grant.decision_id,
    )
}

/// The digest aggregate a grant belongs to: the identity the activation
/// guard protects. The separators stay inside the guard scope's safe
/// charset; an account or repository outside it is refused, which is the
/// closed side of a name the guard cannot key.
pub fn aggregate_key(account: &str, repository: &str, digest: &ContentDigest) -> String {
    format!(
        "{account}/{repository}/{algorithm}/{value}",
        algorithm = digest.algorithm,
        value = digest.value
    )
}

/// The N-Quads that write one grant row as a blank node.
fn grant_nquads(grant: &TrustGrant, node: &str) -> String {
    let mut quads = format!(
        "{node} <dgraph.type> \"{GRANT_TYPE}\" .\n\
         {node} <hodei.trust.grant.key> {key} .\n\
         {node} <hodei.trust.grant.account> {account} .\n\
         {node} <hodei.trust.grant.repository> {repository} .\n\
         {node} <hodei.trust.grant.digest_algorithm> {algorithm} .\n\
         {node} <hodei.trust.grant.digest> {value} .\n\
         {node} <hodei.trust.grant.decision_id> {decision} .\n\
         {node} <hodei.trust.grant.actor> {actor} .\n\
         {node} <hodei.trust.grant.granted_at> {granted_at} .\n",
        key = dql_string(&grant_key(grant)),
        account = dql_string(&grant.account),
        repository = dql_string(&grant.repository),
        algorithm = dql_string(&grant.digest.algorithm.to_string()),
        value = dql_string(&grant.digest.value),
        decision = dql_string(&grant.decision_id),
        actor = dql_string(&grant.actor),
        granted_at = dql_string(&rfc3339(grant.granted_at)),
    );
    let mut optional = |predicate: &str, text: &Option<String>| {
        if let Some(value) = text {
            quads.push_str(&format!("{node} <{predicate}> {} .\n", dql_string(value)));
        }
    };
    optional("hodei.trust.grant.subject", &grant.subject);
    optional("hodei.trust.grant.policy_version", &grant.policy_version);
    optional("hodei.trust.grant.capsule_hash", &grant.capsule_hash);
    optional("hodei.trust.grant.revoke_reason", &grant.revoke_reason);
    optional("hodei.trust.grant.revoked_by", &grant.revoked_by);
    if let Some(revoked_at) = grant.revoked_at {
        quads.push_str(&format!(
            "{node} <hodei.trust.grant.revoked_at> {} .\n",
            dql_string(&rfc3339(revoked_at))
        ));
    }
    if let Some(risk_version) = grant.risk_version {
        quads.push_str(&format!(
            "{node} <hodei.trust.grant.risk_version> \"{risk_version}\" .\n"
        ));
    }
    if let Some(valid_until) = grant.valid_until {
        quads.push_str(&format!(
            "{node} <hodei.trust.grant.valid_until> {} .\n",
            dql_string(&rfc3339(valid_until))
        ));
    }
    quads
}

/// The N-Quads that write one audit event of the ledger, linked to the
/// grant row it speaks about. The event rides the SAME transaction as
/// the change it records.
fn event_nquads(
    kind: &str,
    key: &str,
    grant: &TrustGrant,
    reason: Option<&str>,
    at: DateTime<Utc>,
    grant_node: &str,
    event_node: &str,
) -> String {
    let payload = serde_json::json!({
        "kind": kind,
        "grant": grant_key(grant),
        "account": grant.account,
        "repository": grant.repository,
        "digest": grant.digest.to_string(),
        "decision_id": grant.decision_id,
        "actor": grant.actor,
        "reason": reason,
        "at": rfc3339(at),
    })
    .to_string();
    let mut quads = format!(
        "{event_node} <dgraph.type> \"{EVENT_TYPE}\" .\n\
         {event_node} <hodei.trust.event.key> {key} .\n\
         {event_node} <hodei.trust.event.kind> {kind} .\n\
         {event_node} <hodei.trust.event.account> {account} .\n\
         {event_node} <hodei.trust.event.decision_id> {decision} .\n\
         {event_node} <hodei.trust.event.digest> {digest} .\n\
         {event_node} <hodei.trust.event.actor> {actor} .\n\
         {event_node} <hodei.trust.event.at> {at} .\n\
         {event_node} <hodei.trust.event.payload> {payload} .\n\
         {event_node} <hodei.trust.event.grant> {grant_node} .\n",
        key = dql_string(key),
        kind = dql_string(kind),
        account = dql_string(&grant.account),
        decision = dql_string(&grant.decision_id),
        digest = dql_string(&grant.digest.to_string()),
        actor = dql_string(&grant.actor),
        at = dql_string(&rfc3339(at)),
        payload = dql_string(&payload),
    );
    if !grant.repository.is_empty() {
        quads.push_str(&format!(
            "{event_node} <hodei.trust.event.repository> {} .\n",
            dql_string(&grant.repository)
        ));
    }
    if let Some(reason) = reason {
        quads.push_str(&format!(
            "{event_node} <hodei.trust.event.reason> {} .\n",
            dql_string(reason)
        ));
    }
    quads
}

/// The errors a Dgraph answer may carry inside its JSON: a query that
/// the server refused is NOT an empty answer, it is a failure. Reading a
/// failure as "no grant" would close the channel with the WRONG side --
/// inexistent instead of unavailable -- and the ledger does not guess.
fn require_no_errors(json: &Value) -> Result<(), DgraphTrustError> {
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
    Err(DgraphTrustError::Unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

/// Runs one DQL query and answers its data, refusing the error arrays
/// the wire may carry.
async fn query_json(client: &GraphClient, query: &str) -> Result<Value, DgraphTrustError> {
    let response = client
        .execute_dql(query)
        .await
        .map_err(DgraphTrustError::unavailable)?;
    require_no_errors(&response.json)?;
    Ok(response.json)
}

/// Applies one upsert transaction built per attempt, retrying the
/// conflicts the server reports. A retry re-runs the whole transaction:
/// the query inside it revalidates the state, so a retry never writes
/// over a change it did not see.
async fn upsert_retrying(
    client: &GraphClient,
    build: &(dyn Fn() -> GraphUpsertTransaction + Send + Sync),
) -> Result<Value, DgraphTrustError> {
    let mut last = String::new();
    for attempt in 1..=MAX_WRITE_ATTEMPTS {
        match client.upsert(build()).await {
            Ok(response) => {
                // A server-side refusal travels inside the JSON: it is
                // never read as a successful no-op.
                match require_no_errors(&response.json) {
                    Ok(()) => return Ok(response.json),
                    Err(error) => return Err(error),
                }
            }
            Err(GraphClientError::UpsertConflict { detail }) if attempt < MAX_WRITE_ATTEMPTS => {
                last = detail;
                tokio::time::sleep(RETRY_BASE * attempt as u32).await;
            }
            Err(error) => return Err(DgraphTrustError::unavailable(error)),
        }
    }
    Err(DgraphTrustError::Unavailable(format!(
        "the write kept conflicting after {MAX_WRITE_ATTEMPTS} attempts: {last}"
    )))
}

/// The count a query variable named in the transaction's own answer.
fn bound_count(json: &Value, variable: &str) -> usize {
    json.get(variable)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

/// Applies the additive DDL slice of this adapter. Idempotent: the same
/// text sent twice changes nothing on the server. Bounded retries: two
/// stores opening at once alter the same additive slice, and Dgraph
/// refuses one of them while the indexes settle.
pub async fn ensure_trust_schema(client: &GraphClient) -> Result<(), DgraphTrustError> {
    let mut last = String::new();
    for attempt in 1..=MAX_WRITE_ATTEMPTS {
        match client.alter_schema(TRUST_SCHEMA_DQL).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < MAX_WRITE_ATTEMPTS => {
                last = error.to_string();
                tokio::time::sleep(RETRY_BASE * attempt as u32).await;
            }
            Err(error) => return Err(DgraphTrustError::unavailable(error)),
        }
    }
    Err(DgraphTrustError::Unavailable(format!(
        "the DDL slice did not settle after {MAX_WRITE_ATTEMPTS} attempts: {last}"
    )))
}

/// The result of one guarded activation of a grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantActivation {
    /// The aggregate revision AFTER the activation, read back from the
    /// server: the version the aggregate has reached.
    pub revision: u64,
    /// True when the operation had already committed (a replay): the
    /// grant was already active, no new state was written.
    pub replayed: bool,
    /// What a replay records as its committed answer: the decision the
    /// aggregate's activation first committed.
    pub recorded_result: String,
}

/// The parent conjunct every extra block of a guarded write is gated on,
/// derived from the revision the attempt's pre-read observed: a fresh
/// scope runs the create branch (the guard binds as `g`), an existing
/// one the advance branch (the guard binds as `gv`, filtered by the
/// expected revision).
fn parent_gate(expected_revision: u64) -> &'static str {
    if expected_revision == 0 {
        "eq(len(op), 0) AND eq(len(g), 0)"
    } else {
        "eq(len(op), 0) AND eq(len(gv), 1)"
    }
}

/// The book of grants and the store of decisions over the graph.
pub struct DgraphTrustStore {
    client: Arc<GraphClient>,
    guards: GuardStore,
    tenant: Tenant,
}

impl DgraphTrustStore {
    /// Opens the store on a connected client: the versioned schema boot
    /// runs first (an incompatible server is refused), then this
    /// adapter's additive DDL slice. No store exists on a graph the
    /// ledger cannot trust.
    pub async fn open(client: Arc<GraphClient>, tenant: Tenant) -> Result<Self, DgraphTrustError> {
        let guards = GuardStore::open(client.clone()).await?;
        ensure_trust_schema(&client).await?;
        Ok(Self {
            client,
            guards,
            tenant,
        })
    }

    /// The schema boot report the opening validated.
    pub fn boot_report(&self) -> &hodei_graph_client::schema::SchemaBootReport {
        self.guards.boot_report()
    }

    /// The revision the activation guard of one digest aggregate has
    /// reached. An aggregate nobody activated reads as zero: the claim
    /// `At(0)` is the claim of the first activation.
    pub async fn aggregate_revision(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
    ) -> Result<u64, DgraphTrustError> {
        let scope = self.aggregate_scope(account, repository, digest)?;
        self.guards
            .guard_revision(&scope)
            .await
            .map_err(DgraphTrustError::from)
    }

    /// Activates one grant with the CAS of the activation guard: the
    /// aggregate must be exactly at `expected_revision` for the grant to
    /// land. Two writers activating different decisions on the same
    /// aggregate cannot both confirm: the winner commits (guard advance,
    /// grant row and event are one fact), the loser receives a typed
    /// rejection with the revision the aggregate actually has, and reads
    /// the aggregate to say who won.
    ///
    /// The grant row and its event ride the guarded transaction as extra
    /// blocks gated on the row being absent: a row that already exists
    /// (the same decision granted through the bulk path) is not
    /// duplicated, and the activation still records its version.
    pub async fn activate_grant_expecting_revision(
        &self,
        grant: &TrustGrant,
        expected_revision: u64,
    ) -> Result<GrantActivation, DgraphTrustError> {
        let scope = self.aggregate_scope(&grant.account, &grant.repository, &grant.digest)?;
        let operation = OperationId::new(&format!(
            "grant-{aggregate}-{decision}",
            aggregate = aggregate_key(&grant.account, &grant.repository, &grant.digest),
            decision = grant.decision_id,
        ))
        .map_err(|error| DgraphTrustError::Corrupt(error.to_string()))?;
        let parent = parent_gate(expected_revision);
        let now = Utc::now();
        let effect = GuardedEffect {
            query_lines: vec![format!(
                "gk as var(func: eq(hodei.trust.grant.key, {}))",
                dql_string(&grant_key(grant))
            )],
            set_nquads: String::new(),
            recorded_result: grant.decision_id.clone(),
        };
        let row = grant_nquads(grant, "_:grantrow");
        let event = event_nquads(
            "granted",
            &format!("granted|{}", grant_key(grant)),
            grant,
            None,
            now,
            "_:grantrow",
            "_:grantevent",
        );
        let outcome = self
            .guards
            .execute_guarded_composite(
                &scope,
                &operation,
                RevisionDemand::At(expected_revision),
                effect,
                &|_| {
                    vec![GraphMutationBlock::set_if(
                        format!("{parent} AND eq(len(gk), 0)"),
                        format!("{row}{event}"),
                    )]
                },
            )
            .await?;
        Ok(GrantActivation {
            revision: outcome.guard_revision,
            replayed: outcome.replayed,
            recorded_result: outcome.recorded_result,
        })
    }

    /// The guard scope of one digest aggregate. A name outside the safe
    /// charset is refused: the ledger does not guess a key it cannot
    /// keep.
    fn aggregate_scope(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
    ) -> Result<GuardScope, DgraphTrustError> {
        GuardScope::new(
            GuardKind::Grant,
            &self.tenant,
            &aggregate_key(account, repository, digest),
        )
        .map_err(|error| DgraphTrustError::Corrupt(error.to_string()))
    }

    /// One active grant of that digest **at `at`**, read NOW.
    ///
    /// The query is linearizable (Dgraph's default: never `best_effort`)
    /// and carries no TTL: a revocation whose commit is visible is
    /// answered by every read that starts after it. The admissible
    /// condition mirrors the Surreal contract -- live, and with a
    /// `valid_until` still ahead of `at` -- so a row without a declared
    /// validity is never served. When several versions of one digest are
    /// admissible, the newest decision wins: the versioned ledger serves
    /// the latest grant, not the first it finds.
    async fn read_active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, DgraphTrustError> {
        let query = format!(
            "{{ q(func: eq(hodei.trust.grant.account, {account}), \
                  orderdesc: hodei.trust.grant.granted_at) \
              @filter(eq(hodei.trust.grant.repository, {repository}) AND \
                      eq(hodei.trust.grant.digest_algorithm, {algorithm}) AND \
                      eq(hodei.trust.grant.digest, {value}) AND \
                      not has(hodei.trust.grant.revoked_at) AND \
                      gt(hodei.trust.grant.valid_until, {at})) \
              {{ {fields} }} }}",
            account = dql_string(account),
            repository = dql_string(repository),
            algorithm = dql_string(&digest.algorithm.to_string()),
            value = dql_string(&digest.value),
            at = dql_string(&rfc3339(at)),
            fields = GRANT_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.into_iter().next() {
            None => Ok(None),
            Some(node) => grant_of(&node).map(Some),
        }
    }

    /// Which of the given digests have an active grant in the repository at
    /// `at`, in ONE read bounded by the digests that are asked for.
    ///
    /// The query roots on the exact digest index, so it touches the grants
    /// of those contents and nothing else; the account and the repository
    /// narrow the candidates, and the validity is decided with the domain
    /// rule of the grant itself (`is_active_at`), not with a second copy of
    /// it. Digests are grouped by algorithm because a digest value is only
    /// meaningful next to its algorithm.
    async fn read_active_digests(
        &self,
        account: &str,
        repository: &str,
        digests: &[ContentDigest],
        at: DateTime<Utc>,
    ) -> Result<Vec<ContentDigest>, DgraphTrustError> {
        let mut active: Vec<ContentDigest> = Vec::new();
        let mut by_algorithm: std::collections::BTreeMap<String, Vec<&ContentDigest>> =
            std::collections::BTreeMap::new();
        for digest in digests {
            by_algorithm
                .entry(digest.algorithm.to_string())
                .or_default()
                .push(digest);
        }
        for (algorithm, group) in by_algorithm {
            for chunk in group.chunks(ACTIVE_DIGESTS_CHUNK) {
                let values = chunk
                    .iter()
                    .map(|digest| dql_string(&digest.value))
                    .collect::<Vec<_>>()
                    .join(", ");
                let query = format!(
                    "{{ q(func: eq(hodei.trust.grant.digest, [{values}])) \
                      @filter(eq(hodei.trust.grant.account, {account}) AND \
                              eq(hodei.trust.grant.repository, {repository}) AND \
                              eq(hodei.trust.grant.digest_algorithm, {algorithm}) AND \
                              not has(hodei.trust.grant.revoked_at) AND \
                              gt(hodei.trust.grant.valid_until, {at})) \
                      {{ {fields} }} }}",
                    account = dql_string(account),
                    repository = dql_string(repository),
                    algorithm = dql_string(&algorithm),
                    at = dql_string(&rfc3339(at)),
                    fields = GRANT_FIELDS,
                );
                let response = query_json(&self.client, &query).await?;
                for node in response
                    .get("q")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    let grant = grant_of(node)?;
                    if grant.is_active_at(at) && !active.contains(&grant.digest) {
                        active.push(grant.digest);
                    }
                }
            }
        }
        Ok(active)
    }

    /// Every grant of one repository, active or revoked, in the order
    /// the Surreal contract promises: granted time, then decision, then
    /// digest.
    async fn read_grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, DgraphTrustError> {
        let query = format!(
            "{{ q(func: eq(hodei.trust.grant.account, {account}), \
                  orderasc: hodei.trust.grant.granted_at) \
              @filter(eq(hodei.trust.grant.repository, {repository})) \
              {{ {fields} }} }}",
            account = dql_string(account),
            repository = dql_string(repository),
            fields = GRANT_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        let mut grants: Vec<TrustGrant> = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| grant_of(&node))
            .collect::<Result<_, _>>()?;
        grants.sort_by(|a, b| {
            (a.granted_at, a.decision_id.clone(), a.digest.value.clone()).cmp(&(
                b.granted_at,
                b.decision_id.clone(),
                b.digest.value.clone(),
            ))
        });
        Ok(grants)
    }

    /// Writes the grants of one decision, all or none.
    ///
    /// Every row is keyed and created only when absent: repeating a grant
    /// writes nothing, and a grant that was revoked is NOT reactivated
    /// by repeating it. The granted event of each row rides the same
    /// transaction.
    async fn write_grants(&self, grants: &[TrustGrant]) -> Result<(), DgraphTrustError> {
        if grants.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let mut query = String::from("{\n");
        let mut blocks: Vec<GraphMutationBlock> = Vec::new();
        for (index, grant) in grants.iter().enumerate() {
            // Blank node labels are per request, not per block: two
            // blocks that share `_:grantrow` would write one mixed node.
            // The labels carry the index, so every row of the decision
            // is its own node.
            let row_node = format!("_:grantrow{index}");
            let event_node = format!("_:grantevent{index}");
            let variable = format!("g{index}");
            query.push_str(&format!(
                "  {variable} as var(func: eq(hodei.trust.grant.key, {key}))\n",
                key = dql_string(&grant_key(grant)),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("eq(len({variable}), 0)"),
                format!(
                    "{}{}",
                    grant_nquads(grant, &row_node),
                    event_nquads(
                        "granted",
                        &format!("granted|{}", grant_key(grant)),
                        grant,
                        None,
                        now,
                        &row_node,
                        &event_node,
                    )
                ),
            ));
        }
        query.push('}');
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: blocks.clone(),
            commit_now: true,
        })
        .await?;
        Ok(())
    }

    /// Revokes the live grants of `account` that `filter` names, in ONE
    /// transaction, and returns how many were live.
    ///
    /// The live set is re-read INSIDE the committing transaction and the
    /// write is gated on it being non-empty, so a revocation that raced
    /// another one reports zero and writes nothing: a grant is not
    /// revoked twice, and the count is what this call actually moved.
    /// The revoked event rides the same transaction, linked to every
    /// grant it took back.
    async fn revoke_where(
        &self,
        account: &str,
        filter: &str,
        scope_tag: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, DgraphTrustError> {
        // A var block binds the uids but never travels in the answer:
        // the named block over the same variable is what the response
        // reports, and its length is the count of the grants the
        // transaction saw live.
        let query = format!(
            "{{\n  live as var(func: eq(hodei.trust.grant.account, {account})) \
             @filter({filter} AND not has(hodei.trust.grant.revoked_at))\n  \
             reported(func: uid(live)) {{ uid }}\n}}",
            account = dql_string(account),
            filter = filter,
        );
        let now = dql_string(&rfc3339(at));
        let event_key = dql_string(&format!(
            "revoked|{account}|{scope_tag}|{actor}|{}",
            rfc3339(at)
        ));
        let event = format!(
            "_:event <dgraph.type> \"{EVENT_TYPE}\" .\n\
             _:event <hodei.trust.event.key> {event_key} .\n\
             _:event <hodei.trust.event.kind> \"revoked\" .\n\
             _:event <hodei.trust.event.account> {account} .\n\
             _:event <hodei.trust.event.actor> {actor} .\n\
             _:event <hodei.trust.event.reason> {reason} .\n\
             _:event <hodei.trust.event.at> {now} .\n\
             _:event <hodei.trust.event.payload> {payload} .\n\
             _:event <hodei.trust.event.grant> uid(live) .\n",
            account = dql_string(account),
            actor = dql_string(actor),
            reason = dql_string(reason),
            now = now,
            payload = dql_string(
                &serde_json::json!({
                    "kind": "revoked",
                    "account": account,
                    "scope": scope_tag,
                    "actor": actor,
                    "reason": reason,
                    "at": rfc3339(at),
                })
                .to_string()
            ),
        );
        let update = format!(
            "uid(live) <hodei.trust.grant.revoked_at> {now} .\n\
             uid(live) <hodei.trust.grant.revoked_by> {actor} .\n\
             uid(live) <hodei.trust.grant.revoke_reason> {reason} .\n",
            now = now,
            actor = dql_string(actor),
            reason = dql_string(reason),
        );
        let json = upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![GraphMutationBlock::set_if(
                "gt(len(live), 0)",
                format!("{update}{event}"),
            )],
            commit_now: true,
        })
        .await?;
        Ok(bound_count(&json, "reported") as u64)
    }
}

#[async_trait]
impl TrustLedger for DgraphTrustStore {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        self.write_grants(grants).await.map_err(Into::into)
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
        let filter = format!(
            "eq(hodei.trust.grant.repository, {repository}) AND \
             eq(hodei.trust.grant.decision_id, {decision})",
            repository = dql_string(repository),
            decision = dql_string(decision_id),
        );
        self.revoke_where(
            account,
            &filter,
            &format!("decision:{decision_id}"),
            actor,
            reason,
            at,
        )
        .await
        .map_err(Into::into)
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        let filter = format!(
            "eq(hodei.trust.grant.decision_id, {decision})",
            decision = dql_string(decision_id),
        );
        self.revoke_where(
            account,
            &filter,
            &format!("decision:{decision_id}"),
            actor,
            reason,
            at,
        )
        .await
        .map_err(Into::into)
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
        let filter = format!(
            "eq(hodei.trust.grant.repository, {repository}) AND \
             eq(hodei.trust.grant.digest_algorithm, {algorithm}) AND \
             eq(hodei.trust.grant.digest, {value})",
            repository = dql_string(repository),
            algorithm = dql_string(&digest.algorithm.to_string()),
            value = dql_string(&digest.value),
        );
        self.revoke_where(
            account,
            &filter,
            &format!("digest:{}", digest),
            actor,
            reason,
            at,
        )
        .await
        .map_err(Into::into)
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        self.read_active_grant(account, repository, digest, at)
            .await
            .map_err(Into::into)
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        self.read_grants_of(account, repository)
            .await
            .map_err(Into::into)
    }

    async fn active_digests(
        &self,
        account: &str,
        repository: &str,
        digests: &[ContentDigest],
        at: DateTime<Utc>,
    ) -> Result<Vec<ContentDigest>, CatalogError> {
        self.read_active_digests(account, repository, digests, at)
            .await
            .map_err(Into::into)
    }
}

#[async_trait]
impl GrantSweepSource for DgraphTrustStore {
    async fn unrevoked_grants(&self, limit: usize) -> Result<Vec<GrantSweepRow>, CatalogError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // The server orders and bounds the page, so a pass reads a
        // finite slice of the book whatever its size.
        let query = format!(
            "{{ q(func: type({GRANT_TYPE}), orderasc: hodei.trust.grant.account, \
             orderasc: hodei.trust.grant.repository, orderasc: hodei.trust.grant.digest, \
             first: {limit}) @filter(NOT has(hodei.trust.grant.revoked_at)) {{ {fields} }} }}",
            fields = GRANT_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        let mut rows: Vec<GrantSweepRow> = Vec::new();
        for node in response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            // A node the ledger cannot read back is corruption, and a
            // sweep that skipped it would call the book clean.
            let grant = grant_of(&node)?;
            rows.push(GrantSweepRow {
                decision_id: grant.decision_id,
                account: grant.account,
                repository: grant.repository,
                digest_algorithm: grant.digest.algorithm.to_string(),
                digest: grant.digest.value,
                subject: grant.subject,
                policy_version: grant.policy_version,
                valid_until: grant.valid_until,
            });
        }
        rows.sort_by(|a, b| {
            (&a.account, &a.repository, &a.digest).cmp(&(&b.account, &b.repository, &b.digest))
        });
        Ok(rows)
    }
}

/// The store of promotion decisions over the graph.
///
/// A decision is written once and never changed: the row is keyed by the
/// decision and created only when absent, so writing the same decision
/// twice leaves one decision.
pub struct DgraphDecisionStore {
    client: Arc<GraphClient>,
}

impl DgraphDecisionStore {
    /// A store over a connected client. The schema slice is the ledger's
    /// [`ensure_trust_schema`]: the decisions and the grants share it.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, DgraphTrustError> {
        ensure_trust_schema(&client).await?;
        Ok(Self { client })
    }

    /// The decision a stored node names, or corruption when the payload
    /// does not parse: a decision that cannot be rebuilt is not kept
    /// quiet, it is reported.
    fn decision_of(node: &Value) -> Result<PromotionDecision, DgraphTrustError> {
        let payload = required_text(node, "hodei.trust.decision.payload")?;
        serde_json::from_str(&payload).map_err(|error| {
            DgraphTrustError::Corrupt(format!("a decision does not parse: {error}"))
        })
    }
}

/// The fields a decision read brings back.
const DECISION_FIELDS: &str = "
  uid
  hodei.trust.decision.key
  hodei.trust.decision.payload
";

#[async_trait]
impl DecisionStore for DgraphDecisionStore {
    async fn put(&self, decision: &PromotionDecision) -> Result<(), DecisionStoreError> {
        let key = dql_string(decision.id.as_str());
        let payload = serde_json::to_string(decision)
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        let query = format!("{{\n  d as var(func: eq(hodei.trust.decision.key, {key}))\n}}");
        let quads = format!(
            "_:decision <dgraph.type> \"{DECISION_TYPE}\" .\n\
             _:decision <hodei.trust.decision.key> {key} .\n\
             _:decision <hodei.trust.decision.account> {account} .\n\
             _:decision <hodei.trust.decision.subject> {subject} .\n\
             _:decision <hodei.trust.decision.correlation_id> {correlation} .\n\
             _:decision <hodei.trust.decision.outcome> {outcome} .\n\
             _:decision <hodei.trust.decision.distribution> {distribution} .\n\
             _:decision <hodei.trust.decision.decided_at> {decided_at} .\n\
             _:decision <hodei.trust.decision.payload> {payload} .\n",
            account = dql_string(&decision.request.account),
            subject = dql_string(&decision.request.subject),
            correlation = dql_string(&decision.request.correlation_id),
            outcome = dql_string(decision.outcome.as_str()),
            distribution = dql_string(&decision.request.distribution),
            decided_at = dql_string(&rfc3339(decision.decided_at)),
            payload = dql_string(&payload),
        );
        let json = upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![GraphMutationBlock::set_if("eq(len(d), 0)", quads.clone())],
            commit_now: true,
        })
        .await?;
        let _ = json;
        Ok(())
    }

    async fn get(&self, id: &DecisionId) -> Result<Option<PromotionDecision>, DecisionStoreError> {
        let query = format!(
            "{{ q(func: eq(hodei.trust.decision.key, {key})) {{ {fields} }} }}",
            key = dql_string(id.as_str()),
            fields = DECISION_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => Self::decision_of(&nodes[0]).map(Some).map_err(Into::into),
            n => Err(DecisionStoreError::Unavailable(format!(
                "{n} decision nodes carry the key '{}'",
                id.as_str()
            ))),
        }
    }

    async fn list_for_subject(
        &self,
        account: &str,
        subject: &str,
    ) -> Result<Vec<PromotionDecision>, DecisionStoreError> {
        let query = format!(
            "{{ q(func: eq(hodei.trust.decision.account, {account}), \
                  orderdesc: hodei.trust.decision.decided_at) \
              @filter(eq(hodei.trust.decision.subject, {subject})) {{ {fields} }} }}",
            account = dql_string(account),
            subject = dql_string(subject),
            fields = DECISION_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| Self::decision_of(&node).map_err(Into::into))
            .collect()
    }

    async fn find_by_correlation(
        &self,
        correlation_id: &str,
    ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
        let query = format!(
            "{{ q(func: eq(hodei.trust.decision.correlation_id, {correlation}), \
                  orderdesc: hodei.trust.decision.decided_at) {{ {fields} }} }}",
            correlation = dql_string(correlation_id),
            fields = DECISION_FIELDS,
        );
        let response = query_json(&self.client, &query).await?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.into_iter().next() {
            None => Ok(None),
            Some(node) => Self::decision_of(&node).map(Some).map_err(Into::into),
        }
    }
}

/// The node type of a reactive intent.
pub const INTENT_TYPE: &str = "HodeiReactiveIntent";
/// The node type of an applied effect.
pub const EFFECT_TYPE: &str = "HodeiReactiveEffect";
/// The node type of an execution record.
pub const EXECUTION_TYPE: &str = "HodeiReactiveExecution";
/// The node type of a claim marker: the fact that says WHO took an
/// intent, and at which generation.
pub const CLAIM_TYPE: &str = "HodeiIntentClaim";

/// The key of the claim marker a claim at `generation` must create:
/// unique per intent and per generation, so two executors claiming the
/// same intent at the same generation try to create the SAME marker and
/// the `@upsert` index decides.
fn claim_marker_key(identity: &str, generation: u64) -> String {
    format!("claim|{identity}|{generation}")
}

/// Why the graph refused an intent write. Every variant is closed: the
/// caller knows exactly what was refused and why, and nothing was
/// guessed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DgraphIntentError {
    /// The authority did not answer, or refused the transaction.
    #[error("the intent graph is not available: {0}")]
    Unavailable(String),
    /// A receipt arrived for an intent nobody holds a claim on: the
    /// state it found is named, so the operator can see what stood.
    #[error(
        "the intent {identity} is not claimed (its state is {state}), so nobody can receive this receipt"
    )]
    NotClaimed {
        /// The intent the receipt spoke about.
        identity: String,
        /// The state the intent actually had.
        state: String,
    },
    /// A receipt arrived while the intent is still claimed but its lease
    /// is dead: the claim does not exist any more, whatever the state
    /// field says.
    #[error(
        "the claim of intent {identity} has no live lease, so nobody can receive this receipt"
    )]
    LeaseDead {
        /// The intent the receipt spoke about.
        identity: String,
    },
    /// A fenced write presented a token the intent has moved past: a
    /// worker that was overtaken cannot write.
    #[error(
        "the write of intent {identity} was refused by fencing: the presented token is {presented} and the intent is at generation {current}"
    )]
    FencingRejected {
        /// The intent the write spoke about.
        identity: String,
        /// The token the worker presented.
        presented: u64,
        /// The generation the intent has reached.
        current: u64,
    },
    /// A reconciliation was asked for an intent that is not unknown.
    #[error("the intent {identity} is not unknown, so there is nothing to reconcile")]
    NotUnknown {
        /// The intent the reconciliation asked for.
        identity: String,
    },
    /// The graph contradicts the outbox contract (duplicate rows for one
    /// key, a payload that does not parse).
    #[error("the intent state is corrupt: {0}")]
    Corrupt(String),
}

impl DgraphIntentError {
    fn unavailable(error: impl std::fmt::Display) -> Self {
        Self::Unavailable(error.to_string())
    }
}

impl From<GuardError> for DgraphIntentError {
    fn from(error: GuardError) -> Self {
        DgraphIntentError::Unavailable(error.to_string())
    }
}

impl From<DgraphIntentError> for String {
    fn from(error: DgraphIntentError) -> Self {
        error.to_string()
    }
}

/// One intent node as the store reads it.
#[derive(Debug, Clone)]
struct IntentNode {
    state: String,
    lease_owner: Option<String>,
    lease_until: Option<DateTime<Utc>>,
    generation: u64,
    idempotency_key: String,
    intent: Option<Intent>,
}

/// The fields an intent read brings back.
const INTENT_FIELDS: &str = "
  uid
  hodei.intent.state
  hodei.intent.lease_owner
  hodei.intent.lease_until
  hodei.intent.lease_generation
  hodei.intent.idempotency
  hodei.intent.payload
";

/// The intent a stored node names, or corruption when the payload does
/// not parse: an outbox that cannot rebuild what it said is not kept
/// quiet, it is reported.
fn intent_of(node: &Value) -> Result<Option<Intent>, DgraphIntentError> {
    match text_of(node, "hodei.intent.payload") {
        None => Ok(None),
        Some(payload) => serde_json::from_str(&payload).map(Some).map_err(|error| {
            DgraphIntentError::Corrupt(format!("an intent does not parse: {error}"))
        }),
    }
}

/// The state a stored node says, or corruption when it does not.
fn state_of(node: &Value) -> Result<String, DgraphIntentError> {
    text_of(node, "hodei.intent.state").ok_or_else(|| {
        DgraphIntentError::Corrupt("an intent node without hodei.intent.state".to_string())
    })
}

/// The generation a stored node says: an intent node without one is
/// corrupt, because every intent is born at generation zero.
fn generation_of(node: &Value) -> Result<u64, DgraphIntentError> {
    node.get("hodei.intent.lease_generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            DgraphIntentError::Corrupt(
                "an intent node without hodei.intent.lease_generation".to_string(),
            )
        })
}

/// The instant a stored lease names, when it carries one.
fn lease_until_of(node: &Value) -> Result<Option<DateTime<Utc>>, DgraphIntentError> {
    match text_of(node, "hodei.intent.lease_until") {
        None => Ok(None),
        Some(text) => instant_of(&text)
            .map(Some)
            .map_err(|error| DgraphIntentError::Corrupt(error.to_string())),
    }
}

/// The idempotency key a stored node names, or corruption.
fn idempotency_of(node: &Value) -> Result<String, DgraphIntentError> {
    text_of(node, "hodei.intent.idempotency").ok_or_else(|| {
        DgraphIntentError::Corrupt("an intent node without hodei.intent.idempotency".to_string())
    })
}

/// The N-Quads that write one intent row as a blank node, born pending
/// at generation zero.
fn intent_nquads(intent: &Intent, node: &str) -> String {
    let payload = serde_json::to_string(intent).unwrap_or_else(|_| "null".to_string());
    format!(
        "{node} <dgraph.type> \"{INTENT_TYPE}\" .\n\
         {node} <hodei.intent.key> {key} .\n\
         {node} <hodei.intent.tenant> {tenant} .\n\
         {node} <hodei.intent.state> \"pending\" .\n\
         {node} <hodei.intent.kind> {kind} .\n\
         {node} <hodei.intent.idempotency> {idempotency} .\n\
         {node} <hodei.intent.lease_generation> \"0\" .\n\
         {node} <hodei.intent.payload> {payload} .\n",
        key = dql_string(&intent.identity),
        tenant = dql_string(&intent.tenant),
        kind = dql_string(&intent.kind),
        idempotency = dql_string(&intent.idempotency_key),
        payload = dql_string(&payload),
    )
}

/// The N-Quads that write one applied effect as a blank node, with the
/// fencing token of the claim that applied it inside the fact.
fn effect_nquads(effect: &Effect, generation: u64, node: &str) -> String {
    let payload = serde_json::to_string(&effect.payload).unwrap_or_else(|_| "null".to_string());
    format!(
        "{node} <dgraph.type> \"{EFFECT_TYPE}\" .\n\
         {node} <hodei.effect.key> {key} .\n\
         {node} <hodei.effect.kind> {kind} .\n\
         {node} <hodei.effect.payload> {payload} .\n\
         {node} <hodei.effect.generation> \"{generation}\" .\n",
        key = dql_string(&effect.idempotency_key),
        kind = dql_string(&effect.kind),
        payload = dql_string(&payload),
    )
}

/// The N-Quads that write one execution record as a blank node.
fn execution_nquads(record: &ExecutionRecord, node: &str) -> String {
    let payload = serde_json::to_string(record).unwrap_or_else(|_| "null".to_string());
    format!(
        "{node} <dgraph.type> \"{EXECUTION_TYPE}\" .\n\
         {node} <hodei.execution.key> {key} .\n\
         {node} <hodei.execution.behavior> {behavior} .\n\
         {node} <hodei.execution.event> {event} .\n\
         {node} <hodei.execution.started_at> {started} .\n\
         {node} <hodei.execution.payload> {payload} .\n",
        key = dql_string(&record.key),
        behavior = dql_string(&record.behavior),
        event = dql_string(&record.event_id),
        started = dql_string(&rfc3339(record.started_at)),
        payload = dql_string(&payload),
    )
}

/// The N-Quads that move one bound intent node to `state`, clearing the
/// lease it carried.
fn transition_nquads(uid_var: &str, state: &str, receipt_json: &str) -> String {
    format!(
        "uid({uid_var}) <hodei.intent.state> \"{state}\" .\n\
         uid({uid_var}) <hodei.intent.receipt> {receipt} .\n",
        uid_var = uid_var,
        state = state,
        receipt = dql_string(receipt_json),
    )
}

/// The delete N-Quads that clear the lease of one bound intent node: a
/// terminal or re-marketed intent carries no lease an operator could
/// mistake for a live claim.
fn clear_lease_deletes(uid_var: &str) -> String {
    format!(
        "uid({uid_var}) <hodei.intent.lease_owner> * .\n\
         uid({uid_var}) <hodei.intent.lease_until> * .\n",
        uid_var = uid_var,
    )
}

/// Whether a receipt says something the node ALREADY says: the same
/// receipt twice is a receipt once, exactly as the reference says.
fn receipt_is_already_true(receipt: &Receipt, state: &str) -> bool {
    match receipt {
        Receipt::Applied => state == "applied",
        Receipt::Failed {
            retryable: true, ..
        } => state == "pending",
        Receipt::Failed { .. } => state == "failed",
        Receipt::Unknown { .. } => state == "unknown",
    }
}

/// The receipt a stored node carries, when it carries one.
fn receipt_text(receipt: &Receipt) -> String {
    serde_json::to_string(receipt).unwrap_or_else(|_| "null".to_string())
}

/// The store of reactive intents and their receipts over the graph
/// authority.
///
/// It answers for [`IntentOutbox`] and [`ExecutionStore`] with the same
/// mechanics the ledger uses: keyed rows created only when absent, state
/// transitions gated INSIDE the committing transaction, and every refusal
/// typed. The claim is the compare-and-set; the lease generation is the
/// fencing token; the `Applied` receipt and its effect row are ONE fact.
pub struct DgraphIntentStore {
    client: Arc<GraphClient>,
}

/// The claim a worker won, with the fencing token of the generation its
/// claim wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedIntent {
    /// The intent that was claimed.
    pub intent: Intent,
    /// The monotonic generation the claim wrote: present it back on every
    /// fenced write, and a write behind the generation is refused.
    pub fencing_token: u64,
}

impl DgraphIntentStore {
    /// Opens the store on a connected client: the adapter's additive DDL
    /// slice (the intents share the ledger's) is applied idempotently.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, DgraphTrustError> {
        ensure_trust_schema(&client).await?;
        Ok(Self { client })
    }

    /// The intent node of one identity, or None when nobody said it.
    /// Duplicate nodes for one keyed identity are corruption: the @upsert
    /// index exists so that cannot happen, and a graph that did it anyway
    /// is not read as if it were fine.
    async fn read_intent(&self, identity: &str) -> Result<Option<IntentNode>, DgraphIntentError> {
        let query = format!(
            "{{ q(func: eq(hodei.intent.key, {key})) {{ {fields} }} }}",
            key = dql_string(identity),
            fields = INTENT_FIELDS,
        );
        let response = query_json(&self.client, &query)
            .await
            .map_err(DgraphIntentError::unavailable)?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => {
                let node = &nodes[0];
                Ok(Some(IntentNode {
                    state: state_of(node)?,
                    lease_owner: text_of(node, "hodei.intent.lease_owner"),
                    lease_until: lease_until_of(node)?,
                    generation: generation_of(node)?,
                    idempotency_key: idempotency_of(node)?,
                    intent: intent_of(node)?,
                }))
            }
            n => Err(DgraphIntentError::Corrupt(format!(
                "{n} intent nodes carry the key '{identity}'"
            ))),
        }
    }

    /// Says an intent, born pending at generation zero. Saying it twice
    /// is the same as saying it once: the row is keyed and created only
    /// when absent, so what is already there stays.
    async fn say_intent(&self, intent: &Intent) -> Result<(), DgraphIntentError> {
        let query = format!(
            "{{\n  iv as var(func: eq(hodei.intent.key, {key}))\n}}",
            key = dql_string(&intent.identity),
        );
        let row = intent_nquads(intent, "_:intent");
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![GraphMutationBlock::set_if("eq(len(iv), 0)", row.clone())],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        Ok(())
    }

    /// Takes an intent from `pending` (or from an expired lease) to
    /// `claimed`, with the fencing token of the claim.
    ///
    /// The claimable set is re-read INSIDE the committing transaction and
    /// the transition is gated on it naming exactly the generation the
    /// pre-read saw: an overtaken pre-read writes nothing. Two workers
    /// claiming the same intent write the same state and the same
    /// generation value on the same node, and the `@upsert` index aborts
    /// one of them — the retry re-reads and finds the winner's live
    /// lease. The generation written is always `read + 1` behind the
    /// gate `generation == read`, so it can only grow.
    pub async fn claim_with_token(
        &self,
        identity: &str,
        lease_owner: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimedIntent>, DgraphIntentError> {
        let Some(node) = self.read_intent(identity).await? else {
            return Ok(None);
        };
        let claimable = node.state == "pending"
            || (node.state == "claimed" && node.lease_until.is_some_and(|until| until <= now));
        if !claimable {
            // Applied, failed or unknown: what is already resolved is not
            // reclama; and a live lease belongs to its owner.
            return Ok(None);
        }
        let expected_generation = node.generation;
        let next_generation = expected_generation + 1;
        // The claimable condition is the PRE-READ state, said exactly,
        // WITH the generation the pre-read saw: a `pending` claim does
        // not touch a claimed lease, a claim that re-markets an expired
        // one demands the lease to be dead INSIDE the transaction, and
        // an overtaken generation matches nothing. The claim marker —
        // keyed by intent and generation, created only when absent — is
        // what forces the conflict: two executors claiming the same
        // intent at the same generation try to create the SAME marker
        // and the `@upsert` index aborts one of them. The retry re-reads
        // the winner's live lease and the loser says so.
        let claimable_condition = match node.state.as_str() {
            "pending" => "eq(hodei.intent.state, \"pending\")".to_owned(),
            _ => format!(
                "eq(hodei.intent.state, \"claimed\") AND le(hodei.intent.lease_until, {now})",
                now = dql_string(&rfc3339(now)),
            ),
        };
        let query = format!(
            "{{\n  claimable as var(func: eq(hodei.intent.key, {key})) \
             @filter({condition} AND eq(hodei.intent.lease_generation, {expected_generation}))\n  \
             marker as var(func: eq(hodei.claim.key, {marker_key}))\n}}",
            key = dql_string(identity),
            condition = claimable_condition,
            expected_generation = expected_generation,
            marker_key = dql_string(&claim_marker_key(identity, next_generation)),
        );
        let write = format!(
            "_:claim <dgraph.type> \"{claim_type}\" .\n\
             _:claim <hodei.claim.key> {marker_key} .\n\
             _:claim <hodei.claim.identity> {identity_literal} .\n\
             _:claim <hodei.claim.owner> {owner} .\n\
             _:claim <hodei.claim.at> {at} .\n\
             _:claim <hodei.claim.generation> \"{next}\" .\n\
             _:claim <hodei.claim.intent> uid(claimable) .\n\
             uid(claimable) <hodei.intent.state> \"claimed\" .\n\
             uid(claimable) <hodei.intent.lease_owner> {owner} .\n\
             uid(claimable) <hodei.intent.lease_until> {until} .\n\
             uid(claimable) <hodei.intent.lease_generation> \"{next}\" .\n",
            claim_type = CLAIM_TYPE,
            marker_key = dql_string(&claim_marker_key(identity, next_generation)),
            identity_literal = dql_string(identity),
            owner = dql_string(lease_owner),
            at = dql_string(&rfc3339(Utc::now())),
            until = dql_string(&rfc3339(lease_until)),
            next = next_generation,
        );
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(claimable), 1) AND eq(len(marker), 0)",
                write.clone(),
            )],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        // The state AFTER the commit decides the answer, never the
        // response's snapshot: this claim won only if the live lease is
        // THIS claim's lease at the generation this claim wrote — the
        // loser of the marker race re-reads the winner's lease and says
        // so.
        let after = self.read_intent(identity).await?;
        let won = after.as_ref().is_some_and(|after| {
            after.state == "claimed"
                && after.generation == next_generation
                && after.lease_owner.as_deref() == Some(lease_owner)
        });
        if !won {
            return Ok(None);
        }
        let intent = node.intent.ok_or_else(|| {
            DgraphIntentError::Corrupt(format!("the intent {identity} does not parse"))
        })?;
        Ok(Some(ClaimedIntent {
            intent,
            fencing_token: next_generation,
        }))
    }

    /// Writes the receipt presenting the fencing token of the claim it
    /// answers: the explicit path for a worker that knows WHICH claim it
    /// holds. A token behind the intent's generation is refused with
    /// [`DgraphIntentError::FencingRejected`], whatever the state field
    /// says.
    pub async fn record_receipt_fenced(
        &self,
        identity: &str,
        fencing_token: u64,
        receipt: &Receipt,
    ) -> Result<(), DgraphIntentError> {
        self.answer_claim(identity, Some(fencing_token), receipt, Utc::now())
            .await
    }

    /// Writes the receipt of a claimed intent, presenting the fencing
    /// token of the claim that answers.
    ///
    /// The gate demands the state `claimed`, a LIVE lease, and exactly
    /// the presented generation: a worker whose lease died cannot explain
    /// anything, and a worker whose token was overtaken cannot close
    /// somebody else's claim. Both refusals are typed. An `Applied`
    /// receipt writes the effect row — keyed by the effect ID, created
    /// only when absent — in the SAME transaction: the effect cannot
    /// exist without its receipt, and a receipt cannot exist without its
    /// effect.
    async fn answer_claim(
        &self,
        identity: &str,
        token: Option<u64>,
        receipt: &Receipt,
        now: DateTime<Utc>,
    ) -> Result<(), DgraphIntentError> {
        let Some(node) = self.read_intent(identity).await? else {
            return Err(DgraphIntentError::NotClaimed {
                identity: identity.to_owned(),
                state: "absent".to_owned(),
            });
        };
        if receipt_is_already_true(receipt, &node.state) {
            return Ok(());
        }
        if node.state != "claimed" {
            return Err(DgraphIntentError::NotClaimed {
                identity: identity.to_owned(),
                state: node.state,
            });
        }
        let (target_state, receipt_json) = match receipt {
            Receipt::Applied => ("applied", receipt_text(receipt)),
            Receipt::Failed {
                retryable: true, ..
            } => ("pending", receipt_text(receipt)),
            Receipt::Failed { .. } => ("failed", receipt_text(receipt)),
            Receipt::Unknown { .. } => ("unknown", receipt_text(receipt)),
        };
        // The fence, conjunct by conjunct: the live lease refuses the
        // worker that outlived its claim; the exact token refuses the
        // worker that was overtaken.
        let mut fence = format!(
            "eq(hodei.intent.state, \"claimed\") AND gt(hodei.intent.lease_until, {now})",
            now = dql_string(&rfc3339(now)),
        );
        if let Some(presented) = token {
            fence.push_str(&format!(
                " AND eq(hodei.intent.lease_generation, {presented})"
            ));
        }
        let mut query = format!(
            "{{\n  claimed as var(func: eq(hodei.intent.key, {key})) @filter({fence})\n",
            key = dql_string(identity),
            fence = fence,
        );
        // An `Applied` receipt carries its effect row in the same
        // transaction: the var of the effect's key goes in the query, its
        // quads behind the gate of absence.
        if let Receipt::Applied = receipt {
            query.push_str(&format!(
                "  ef as var(func: eq(hodei.effect.key, {key}))\n",
                key = dql_string(&node.idempotency_key),
            ));
        }
        query.push_str("  answering(func: uid(claimed)) { uid }\n}");
        let mut blocks = Vec::new();
        if let Receipt::Applied = receipt {
            let effect = Effect::new(
                node.intent
                    .as_ref()
                    .map(|one| one.kind.clone())
                    .unwrap_or_default(),
                node.idempotency_key.clone(),
                node.intent
                    .as_ref()
                    .map(|one| one.payload.clone())
                    .unwrap_or(Value::Null),
            );
            blocks.push(GraphMutationBlock::set_if(
                "eq(len(claimed), 1) AND eq(len(ef), 0)",
                effect_nquads(&effect, token.unwrap_or(node.generation), "_:effect"),
            ));
        }
        blocks.push(GraphMutationBlock {
            cond: Some("eq(len(claimed), 1)".to_owned()),
            set_nquads: transition_nquads("claimed", target_state, &receipt_json),
            delete_nquads: clear_lease_deletes("claimed"),
        });
        let json = upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: blocks.clone(),
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        self.assert_answered(identity, token, &json, &node).await
    }

    /// The refusal of a transaction whose gate saw nothing: the typed
    /// error names whether the token was overtaken, the lease died or the
    /// claim was never there, read fresh so the operator sees the state
    /// the intent has NOW.
    async fn assert_answered(
        &self,
        identity: &str,
        token: Option<u64>,
        json: &Value,
        pre_read: &IntentNode,
    ) -> Result<(), DgraphIntentError> {
        let answered = json
            .get("answering")
            .and_then(Value::as_array)
            .map(|nodes| !nodes.is_empty())
            .unwrap_or(false);
        if answered {
            return Ok(());
        }
        let node = self
            .read_intent(identity)
            .await?
            .unwrap_or_else(|| pre_read.clone());
        if let Some(presented) = token {
            if node.generation > presented {
                return Err(DgraphIntentError::FencingRejected {
                    identity: identity.to_owned(),
                    presented,
                    current: node.generation,
                });
            }
        }
        if node.state == "claimed" {
            return Err(DgraphIntentError::LeaseDead {
                identity: identity.to_owned(),
            });
        }
        Err(DgraphIntentError::NotClaimed {
            identity: identity.to_owned(),
            state: node.state,
        })
    }

    /// The intents that are waiting: the pending ones, plus the claimed
    /// ones whose lease died, which are put back on the market first.
    /// The unknown is NOT here: it is not re-executed, it is reconciled. The
    /// re-marketing and the listing are two steps: the query of an upsert
    /// runs BEFORE its mutation, so the ones just put back are read after
    /// the fact, not from a stale snapshot.
    async fn the_pending(&self, now: DateTime<Utc>) -> Result<Vec<Intent>, DgraphIntentError> {
        let re_market_query = format!(
            "{{\n  expired as var(func: eq(hodei.intent.state, \"claimed\")) \
             @filter(le(hodei.intent.lease_until, {now}))\n  \
             re_marketed(func: uid(expired)) {{ uid }}\n}}",
            now = dql_string(&rfc3339(now)),
        );
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: re_market_query.clone(),
            blocks: vec![GraphMutationBlock {
                cond: Some("gt(len(expired), 0)".to_owned()),
                set_nquads: "uid(expired) <hodei.intent.state> \"pending\" .\n".to_owned(),
                delete_nquads: clear_lease_deletes("expired"),
            }],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        let query = format!(
            "{{ q(func: eq(hodei.intent.state, \"pending\")) {{ {fields} }} }}",
            fields = INTENT_FIELDS,
        );
        let response = query_json(&self.client, &query)
            .await
            .map_err(DgraphIntentError::unavailable)?;
        let mut pending: Vec<Intent> = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| {
                intent_of(&node)?.ok_or_else(|| {
                    DgraphIntentError::Corrupt("a pending intent does not parse".to_string())
                })
            })
            .collect::<Result<_, _>>()?;
        pending.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(pending)
    }

    /// Returns an `unknown` intent to `pending`, explicitly: the
    /// reconciliation happened, somebody looked at the real world, and
    /// NOW the intent may be tried again. The receipt it carried goes
    /// away with it: what is reconciled starts anew.
    async fn reconcile_unknown(&self, identity: &str) -> Result<(), DgraphIntentError> {
        let Some(node) = self.read_intent(identity).await? else {
            return Err(DgraphIntentError::NotUnknown {
                identity: identity.to_owned(),
            });
        };
        if node.state != "unknown" {
            return Err(DgraphIntentError::NotUnknown {
                identity: identity.to_owned(),
            });
        }
        let query = format!(
            "{{\n  unknown as var(func: eq(hodei.intent.key, {key})) \
             @filter(eq(hodei.intent.state, \"unknown\"))\n  \
             answering(func: uid(unknown)) {{ uid }}\n}}",
            key = dql_string(identity),
        );
        let block = GraphMutationBlock {
            cond: Some("eq(len(unknown), 1)".to_owned()),
            set_nquads: "uid(unknown) <hodei.intent.state> \"pending\" .\n".to_owned(),
            delete_nquads: format!(
                "uid(unknown) <hodei.intent.receipt> * .\n{}",
                clear_lease_deletes("unknown")
            ),
        };
        let json = upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![block.clone()],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        let answered = json
            .get("answering")
            .and_then(Value::as_array)
            .map(|nodes| !nodes.is_empty())
            .unwrap_or(false);
        if !answered {
            return Err(DgraphIntentError::NotUnknown {
                identity: identity.to_owned(),
            });
        }
        Ok(())
    }

    /// The execution record of one key, when it happened.
    async fn the_execution(&self, key: &str) -> Result<Option<ExecutionRecord>, DgraphIntentError> {
        let query = format!(
            "{{ q(func: eq(hodei.execution.key, {key})) {{ hodei.execution.payload }} }}",
            key = dql_string(key),
        );
        let response = query_json(&self.client, &query)
            .await
            .map_err(DgraphIntentError::unavailable)?;
        let nodes = response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => text_of(&nodes[0], "hodei.execution.payload")
                .and_then(|payload| serde_json::from_str(&payload).ok())
                .map(Some)
                .ok_or_else(|| {
                    DgraphIntentError::Corrupt("an execution record does not parse".to_string())
                }),
            n => Err(DgraphIntentError::Corrupt(format!(
                "{n} execution nodes carry the key '{key}'"
            ))),
        }
    }

    /// Writes an execution down. The record is keyed and the write
    /// REPLACES: the same key twice is one record, the one written last,
    /// exactly as the reference's UPSERT CONTENT.
    async fn write_execution(&self, record: &ExecutionRecord) -> Result<(), DgraphIntentError> {
        let query = format!(
            "{{\n  ex as var(func: eq(hodei.execution.key, {key}))\n}}",
            key = dql_string(&record.key),
        );
        let fresh = execution_nquads(record, "_:execution");
        let mut update = format!(
            "uid(ex) <hodei.execution.behavior> {behavior} .\n\
             uid(ex) <hodei.execution.event> {event} .\n\
             uid(ex) <hodei.execution.started_at> {started} .\n\
             uid(ex) <hodei.execution.payload> {payload} .\n",
            behavior = dql_string(&record.behavior),
            event = dql_string(&record.event_id),
            started = dql_string(&rfc3339(record.started_at)),
            payload =
                dql_string(&serde_json::to_string(record).unwrap_or_else(|_| "null".to_string())),
        );
        update.push_str(&format!(
            "uid(ex) <hodei.execution.key> {key} .\n",
            key = dql_string(&record.key),
        ));
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(ex), 1)", update.clone()),
                GraphMutationBlock::set_if("eq(len(ex), 0)", fresh.clone()),
            ],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        Ok(())
    }

    /// Records that an effect was applied, keyed by the effect ID and
    /// created only when absent: recording it twice is recording it once.
    async fn write_effect(&self, effect: &Effect) -> Result<(), DgraphIntentError> {
        let query = format!(
            "{{\n  ef as var(func: eq(hodei.effect.key, {key}))\n}}",
            key = dql_string(&effect.idempotency_key),
        );
        let row = effect_nquads(effect, 0, "_:effect");
        upsert_retrying(&self.client, &|| GraphUpsertTransaction {
            query: query.clone(),
            blocks: vec![GraphMutationBlock::set_if("eq(len(ef), 0)", row.clone())],
            commit_now: true,
        })
        .await
        .map_err(DgraphIntentError::unavailable)?;
        Ok(())
    }

    /// The effects that have already been applied, by their key.
    async fn the_effects(&self) -> Result<Vec<Effect>, DgraphIntentError> {
        let query = format!(
            "{{ q(func: type({EFFECT_TYPE}), orderasc: hodei.effect.key) {{ hodei.effect.key hodei.effect.kind hodei.effect.payload }} }}"
        );
        let response = query_json(&self.client, &query)
            .await
            .map_err(DgraphIntentError::unavailable)?;
        response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| {
                let key = text_of(&node, "hodei.effect.key").ok_or_else(|| {
                    DgraphIntentError::Corrupt("an effect node without its key".to_string())
                })?;
                let kind = text_of(&node, "hodei.effect.kind").unwrap_or_default();
                let payload = text_of(&node, "hodei.effect.payload")
                    .and_then(|payload| serde_json::from_str(&payload).ok())
                    .unwrap_or(Value::Null);
                Ok(Effect::new(kind, key, payload))
            })
            .collect()
    }

    /// The executions of the store, the oldest first.
    async fn the_executions(&self) -> Result<Vec<ExecutionRecord>, DgraphIntentError> {
        let query = format!(
            "{{ q(func: type({EXECUTION_TYPE}), orderasc: hodei.execution.started_at) {{ hodei.execution.payload }} }}"
        );
        let response = query_json(&self.client, &query)
            .await
            .map_err(DgraphIntentError::unavailable)?;
        response
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| {
                text_of(&node, "hodei.execution.payload")
                    .and_then(|payload| serde_json::from_str(&payload).ok())
                    .ok_or_else(|| {
                        DgraphIntentError::Corrupt("an execution record does not parse".to_string())
                    })
            })
            .collect()
    }
}

#[async_trait]
impl IntentOutbox for DgraphIntentStore {
    async fn put_pending(&self, intent: &Intent) -> Result<(), String> {
        self.say_intent(intent).await.map_err(Into::into)
    }

    async fn claim(
        &self,
        identity: &str,
        lease_owner: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<Intent>, String> {
        self.claim_with_token(identity, lease_owner, lease_until, now)
            .await
            .map(|claimed| claimed.map(|one| one.intent))
            .map_err(Into::into)
    }

    async fn record_receipt(&self, identity: &str, receipt: &Receipt) -> Result<(), String> {
        self.answer_claim(identity, None, receipt, Utc::now())
            .await
            .map_err(Into::into)
    }

    async fn recover_pending(&self, now: DateTime<Utc>) -> Result<Vec<Intent>, String> {
        self.the_pending(now).await.map_err(Into::into)
    }

    async fn reconcile(&self, identity: &str) -> Result<(), String> {
        self.reconcile_unknown(identity).await.map_err(Into::into)
    }
}

#[async_trait]
impl ExecutionStore for DgraphIntentStore {
    async fn execution_of(&self, key: &str) -> Option<ExecutionRecord> {
        self.the_execution(key).await.ok().flatten()
    }

    async fn put(&self, record: ExecutionRecord) {
        let _ = self.write_execution(&record).await;
    }

    async fn applied_effects(&self) -> Vec<Effect> {
        self.the_effects().await.unwrap_or_default()
    }

    async fn record_effect(&self, effect: Effect) {
        let _ = self.write_effect(&effect).await;
    }

    async fn executions(&self) -> Vec<ExecutionRecord> {
        let mut records = self.the_executions().await.unwrap_or_default();
        records.sort_by_key(|record| record.started_at);
        records
    }
}
