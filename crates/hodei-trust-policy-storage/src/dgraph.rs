//! The trust grants and the promotion decisions over the graph authority.
//!
//! This adapter sits beside the Surreal ones, not instead of them: the
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

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphClientError, GuardError, GuardKind, GuardScope, GuardStore, GuardedEffect,
    OperationId, RevisionDemand, Tenant,
};
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
