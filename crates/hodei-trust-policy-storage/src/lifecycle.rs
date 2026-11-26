//! The lifecycle of policy sets over the graph authority: revisions, the
//! head that decides and the audit events, per account.
//!
//! This is the persistence of [`hodei_trust_policy::lifecycle`], behind the
//! [`LifecycleStore`] port. It carries the property the domain promises: an
//! activation is a **compare-and-set inside one transaction**.
//!
//! - **One guard per (account, policy set).** Every write of a set advances
//!   the same [`GuardKind::PolicyHead`] guard node, demanding the exact
//!   revision the attempt read ([`RevisionDemand::At`]). Two writers that
//!   read the same state cannot both commit: the server aborts one, the
//!   loser re-reads, and what it finds decides its answer.
//! - **The head is compared by the revision the caller expects.** The head
//!   read happens after the guard read, so a head newer than the guard
//!   revision makes the demand fail instead of passing; and the head block
//!   is additionally gated INSIDE the transaction on the head still holding
//!   the expected sequence. A caller that expected another head is told
//!   [`LifecycleError::StaleHead`] and nothing is written.
//! - **The effect rides the guard.** Revision, head and events are extra
//!   blocks of the guarded transaction: the guard advance, the operation
//!   marker and the lifecycle rows are one fact.
//! - **Tenancy is in the key AND in the query.** Every row key starts with
//!   the account, every row carries it as an exact-indexed predicate, and
//!   every read filters on it: a set of another account is not "not found",
//!   it is invisible.
//!
//! The DDL slice is additive and idempotent, applied at open.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_graph_client::port::GraphMutationBlock;
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GuardError, GuardKind, GuardScope, GuardStore, GuardedEffect, OperationId,
    RevisionDemand, Tenant,
};
use hodei_trust_policy::lifecycle::{
    activation_event, invalidation_event, saved_event, validate_policy, Actors, Head,
    LifecycleError, LifecycleEvent, PolicyRevision, SeparationPolicy, MAX_REVISIONS,
    ROLLBACK_PREFIX,
};
use hodei_trust_policy::policy::TrustPolicy;
use hodei_trust_policy::ports::LifecycleStore;
use serde_json::Value;

use crate::dgraph::{alter_schema_retrying, parent_gate, query_json, rfc3339, text_of};

/// The node type of a policy revision.
pub const REVISION_TYPE: &str = "HodeiPolicyRevision";
/// The node type of a policy head.
pub const HEAD_TYPE: &str = "HodeiPolicyHead";
/// The node type of a lifecycle audit event.
pub const EVENT_TYPE: &str = "HodeiPolicyLifecycleEvent";

/// The DDL slice of the lifecycle: additive predicates and types.
pub const LIFECYCLE_SCHEMA_DQL: &str = "
hodei.policy.revision.key: string @index(exact) @upsert .
hodei.policy.revision.tenant: string @index(exact) .
hodei.policy.revision.policy_id: string @index(exact) .
hodei.policy.revision.sequence: int @index(int) .
hodei.policy.revision.digest: string @index(exact) .
hodei.policy.revision.saved_by: string .
hodei.policy.revision.saved_at: datetime .
hodei.policy.revision.reason: string .
hodei.policy.revision.json: string .
hodei.policy.head.key: string @index(exact) @upsert .
hodei.policy.head.tenant: string @index(exact) .
hodei.policy.head.policy_id: string @index(exact) .
hodei.policy.head.sequence: int @index(int) .
hodei.policy.head.digest: string .
hodei.policy.head.json: string .
hodei.policy.event.key: string @index(exact) @upsert .
hodei.policy.event.tenant: string @index(exact) .
hodei.policy.event.policy_id: string @index(exact) .
hodei.policy.event.kind: string @index(exact) .
hodei.policy.event.order: int @index(int) .
hodei.policy.event.at: datetime .
hodei.policy.event.json: string .
type HodeiPolicyRevision {
  hodei.policy.revision.key
  hodei.policy.revision.tenant
  hodei.policy.revision.policy_id
  hodei.policy.revision.sequence
  hodei.policy.revision.digest
  hodei.policy.revision.saved_by
  hodei.policy.revision.saved_at
  hodei.policy.revision.reason
  hodei.policy.revision.json
}
type HodeiPolicyHead {
  hodei.policy.head.key
  hodei.policy.head.tenant
  hodei.policy.head.policy_id
  hodei.policy.head.sequence
  hodei.policy.head.digest
  hodei.policy.head.json
}
type HodeiPolicyLifecycleEvent {
  hodei.policy.event.key
  hodei.policy.event.tenant
  hodei.policy.event.policy_id
  hodei.policy.event.kind
  hodei.policy.event.order
  hodei.policy.event.at
  hodei.policy.event.json
}
";

/// How many times a save re-reads after losing the guard: ten simultaneous
/// drafts need ten rounds' worth of churn, and losing a draft to a busy
/// store would be a worse answer than waiting for it.
const SAVE_ATTEMPTS: usize = 64;
/// The activation and the rollback lose to a head change (answered) or to
/// a save that moved the guard (retried).
const MOVE_ATTEMPTS: usize = 48;

fn unavailable(error: impl std::fmt::Display) -> LifecycleError {
    LifecycleError::Unavailable(error.to_string())
}

/// The lifecycle store over the graph authority.
pub struct DgraphLifecycleStore {
    client: Arc<GraphClient>,
    guards: GuardStore,
    separation: SeparationPolicy,
}

/// What one attempt of a guarded move reads before it writes.
struct Observed {
    /// The guard revision read FIRST.
    guard: u64,
    /// The head read after it.
    head: Option<Head>,
}

impl DgraphLifecycleStore {
    /// Opens the store on a connected client: the versioned schema boot
    /// runs first, then the additive DDL slice of the lifecycle.
    pub async fn open(
        client: Arc<GraphClient>,
        separation: SeparationPolicy,
    ) -> Result<Self, LifecycleError> {
        let guards = GuardStore::open(client.clone())
            .await
            .map_err(unavailable)?;
        alter_schema_retrying(&client, LIFECYCLE_SCHEMA_DQL)
            .await
            .map_err(unavailable)?;
        Ok(Self {
            client,
            guards,
            separation,
        })
    }

    /// The separation of editor and approver this store enforces.
    pub fn separation(&self) -> SeparationPolicy {
        self.separation
    }

    fn tenant(account: &str) -> Result<Tenant, LifecycleError> {
        Tenant::new(account).map_err(|error| {
            LifecycleError::InvalidPolicy(
                String::new(),
                format!("the account is not valid: {error}"),
            )
        })
    }

    fn scope(tenant: &Tenant, policy_id: &str) -> Result<GuardScope, LifecycleError> {
        GuardScope::new(GuardKind::PolicyHead, tenant, policy_id).map_err(|error| {
            LifecycleError::InvalidPolicy(policy_id.to_string(), error.to_string())
        })
    }

    async fn guard_revision(&self, scope: &GuardScope) -> Result<u64, LifecycleError> {
        self.guards.guard_revision(scope).await.map_err(unavailable)
    }

    /// Reads the guard FIRST and the head second: a head newer than the
    /// guard revision makes the guarded demand fail, never pass.
    async fn observe(
        &self,
        tenant: &Tenant,
        scope: &GuardScope,
        policy_id: &str,
    ) -> Result<Observed, LifecycleError> {
        let guard = self.guard_revision(scope).await?;
        let head = self.read_head(tenant, policy_id).await?;
        Ok(Observed { guard, head })
    }

    async fn read_head(
        &self,
        tenant: &Tenant,
        policy_id: &str,
    ) -> Result<Option<Head>, LifecycleError> {
        let query = format!(
            "{{ q(func: eq(hodei.policy.head.key, {key})) \
             @filter(eq(hodei.policy.head.tenant, {tenant})) {{ hodei.policy.head.json }} }}",
            key = dql_string(&head_key(tenant, policy_id)),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        match nodes.len() {
            0 => Ok(None),
            1 => parse_json(&nodes[0], "hodei.policy.head.json").map(Some),
            n => Err(LifecycleError::Unavailable(format!(
                "{n} head nodes carry the key of {policy_id}"
            ))),
        }
    }

    async fn last_saved(&self, tenant: &Tenant, policy_id: &str) -> Result<u64, LifecycleError> {
        let query = format!(
            "{{ q(func: eq(hodei.policy.revision.policy_id, {policy}), \
             orderdesc: hodei.policy.revision.sequence, first: 1) \
             @filter(eq(hodei.policy.revision.tenant, {tenant})) \
             {{ hodei.policy.revision.sequence }} }}",
            policy = dql_string(policy_id),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        Ok(nodes
            .first()
            .and_then(|node| node.get("hodei.policy.revision.sequence"))
            .and_then(Value::as_u64)
            .unwrap_or(0))
    }

    /// Runs one guarded move: the guard demanded at `observed`, the extra
    /// block carrying the lifecycle rows.
    async fn guarded(
        &self,
        scope: &GuardScope,
        kind: &str,
        observed: u64,
        query_lines: Vec<String>,
        block: GraphMutationBlock,
    ) -> Result<Result<(), GuardError>, LifecycleError> {
        let operation = OperationId::new(&operation_id(kind)).map_err(unavailable)?;
        let effect = GuardedEffect {
            query_lines,
            set_nquads: String::new(),
            recorded_result: kind.to_string(),
        };
        let outcome = self
            .guards
            .execute_guarded_composite(
                scope,
                &operation,
                RevisionDemand::At(observed),
                effect,
                &|_| vec![block.clone()],
            )
            .await;
        match outcome {
            Ok(done) if done.replayed => Err(LifecycleError::Unavailable(
                "the operation marker was already committed".to_string(),
            )),
            Ok(_) => Ok(Ok(())),
            Err(error @ GuardError::CasRejected { .. }) => Ok(Err(error)),
            Err(error) => Err(unavailable(error)),
        }
    }
}

fn operation_id(kind: &str) -> String {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!(
        "policy-{kind}-{nanos:x}-{pid:x}-{n:x}",
        pid = std::process::id(),
        n = NONCE.fetch_add(1, Ordering::SeqCst)
    )
}

fn revision_key(tenant: &Tenant, policy_id: &str, sequence: u64) -> String {
    format!("{}|{policy_id}|{sequence}", tenant.as_str())
}

fn head_key(tenant: &Tenant, policy_id: &str) -> String {
    format!("{}|{policy_id}", tenant.as_str())
}

fn event_key(tenant: &Tenant, policy_id: &str, order: u64) -> String {
    format!("{}|{policy_id}|{order}", tenant.as_str())
}

fn rows(json: &Value, name: &str) -> Vec<Value> {
    json.get(name)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn parse_json<T: serde::de::DeserializeOwned>(
    node: &Value,
    predicate: &str,
) -> Result<T, LifecycleError> {
    let text = text_of(node, predicate)
        .ok_or_else(|| LifecycleError::Unavailable(format!("a node without {predicate}")))?;
    serde_json::from_str(&text).map_err(|error| {
        LifecycleError::Unavailable(format!("{predicate} does not parse: {error}"))
    })
}

fn json_text<T: serde::Serialize>(value: &T) -> Result<String, LifecycleError> {
    serde_json::to_string(value).map_err(unavailable)
}

fn revision_nquads(
    tenant: &Tenant,
    revision: &PolicyRevision,
    node: &str,
) -> Result<String, LifecycleError> {
    Ok(format!(
        "{node} <dgraph.type> \"{REVISION_TYPE}\" .\n\
         {node} <hodei.policy.revision.key> {key} .\n\
         {node} <hodei.policy.revision.tenant> {tenant} .\n\
         {node} <hodei.policy.revision.policy_id> {policy} .\n\
         {node} <hodei.policy.revision.sequence> \"{sequence}\" .\n\
         {node} <hodei.policy.revision.digest> {digest} .\n\
         {node} <hodei.policy.revision.saved_by> {saved_by} .\n\
         {node} <hodei.policy.revision.saved_at> {saved_at} .\n\
         {node} <hodei.policy.revision.reason> {reason} .\n\
         {node} <hodei.policy.revision.json> {json} .\n",
        key = dql_string(&revision_key(
            tenant,
            &revision.policy_id,
            revision.sequence
        )),
        tenant = dql_string(tenant.as_str()),
        policy = dql_string(&revision.policy_id),
        sequence = revision.sequence,
        digest = dql_string(&revision.digest),
        saved_by = dql_string(&revision.saved_by),
        saved_at = dql_string(&rfc3339(revision.saved_at)),
        reason = dql_string(&revision.reason),
        json = dql_string(&json_text(revision)?),
    ))
}

/// The head predicates, addressed to a node (`uid(hv)` or a blank node).
fn head_nquads(tenant: &Tenant, head: &Head, node: &str) -> Result<String, LifecycleError> {
    Ok(format!(
        "{node} <dgraph.type> \"{HEAD_TYPE}\" .\n\
         {node} <hodei.policy.head.key> {key} .\n\
         {node} <hodei.policy.head.tenant> {tenant} .\n\
         {node} <hodei.policy.head.policy_id> {policy} .\n\
         {node} <hodei.policy.head.sequence> \"{sequence}\" .\n\
         {node} <hodei.policy.head.digest> {digest} .\n\
         {node} <hodei.policy.head.json> {json} .\n",
        key = dql_string(&head_key(tenant, &head.policy_id)),
        tenant = dql_string(tenant.as_str()),
        policy = dql_string(&head.policy_id),
        sequence = head.sequence,
        digest = dql_string(&head.digest),
        json = dql_string(&json_text(head)?),
    ))
}

/// The events of one operation, ordered by the guard revision the
/// operation reaches and their index inside it.
fn events_nquads(
    tenant: &Tenant,
    reached: u64,
    events: &[LifecycleEvent],
) -> Result<String, LifecycleError> {
    let mut quads = String::new();
    for (index, event) in events.iter().enumerate() {
        let order = reached * 10 + index as u64;
        let node = format!("_:event{index}");
        quads.push_str(&format!(
            "{node} <dgraph.type> \"{EVENT_TYPE}\" .\n\
             {node} <hodei.policy.event.key> {key} .\n\
             {node} <hodei.policy.event.tenant> {tenant} .\n\
             {node} <hodei.policy.event.policy_id> {policy} .\n\
             {node} <hodei.policy.event.kind> {kind} .\n\
             {node} <hodei.policy.event.order> \"{order}\" .\n\
             {node} <hodei.policy.event.at> {at} .\n\
             {node} <hodei.policy.event.json> {json} .\n",
            key = dql_string(&event_key(tenant, &event.policy_id, order)),
            tenant = dql_string(tenant.as_str()),
            policy = dql_string(&event.policy_id),
            kind = dql_string(&event.kind),
            at = dql_string(&rfc3339(event.at)),
            json = dql_string(&json_text(event)?),
        ));
    }
    Ok(quads)
}

/// The in-transaction lookup of a revision row by key.
fn revision_lookup(var: &str, tenant: &Tenant, policy_id: &str, sequence: u64) -> String {
    format!(
        "{var} as var(func: eq(hodei.policy.revision.key, {}))",
        dql_string(&revision_key(tenant, policy_id, sequence))
    )
}

fn stale(policy_id: &str, expected: Option<u64>, current: Option<u64>) -> LifecycleError {
    LifecycleError::StaleHead(
        policy_id.to_string(),
        expected.unwrap_or(0),
        current.unwrap_or(0),
    )
}

/// The lookup that binds the head node ONLY if it still holds the expected
/// sequence inside the transaction, and the gate that requires it.
fn head_gate(
    tenant: &Tenant,
    policy_id: &str,
    expected: Option<u64>,
) -> (String, &'static str, &'static str) {
    let key = dql_string(&head_key(tenant, policy_id));
    match expected {
        None => (
            format!("hv as var(func: eq(hodei.policy.head.key, {key}))"),
            "eq(len(hv), 0)",
            "_:head",
        ),
        Some(sequence) => (
            format!(
                "hv as var(func: eq(hodei.policy.head.key, {key})) \
                 @filter(eq(hodei.policy.head.sequence, {sequence}))"
            ),
            "eq(len(hv), 1)",
            "uid(hv)",
        ),
    }
}

#[async_trait]
impl LifecycleStore for DgraphLifecycleStore {
    async fn save(
        &self,
        account: &str,
        policy: TrustPolicy,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<PolicyRevision, LifecycleError> {
        validate_policy(&policy)?;
        self.separation.check_editor(actor)?;
        let tenant = Self::tenant(account)?;
        let scope = Self::scope(&tenant, &policy.id)?;

        let mut last = String::new();
        for _ in 0..SAVE_ATTEMPTS {
            // The guard is read BEFORE the sequence: a save that lands in
            // between moves the guard and the demand below fails.
            let observed = self.guard_revision(&scope).await?;
            let sequence = self.last_saved(&tenant, &policy.id).await? + 1;
            if sequence > MAX_REVISIONS {
                return Err(LifecycleError::HistoryFull(policy.id.clone()));
            }
            let revision = PolicyRevision::new(policy.clone(), sequence, actor, at, reason);
            let quads = format!(
                "{}{}",
                revision_nquads(&tenant, &revision, "_:revision")?,
                events_nquads(&tenant, observed + 1, &[saved_event(&revision, at)])?
            );
            let block = GraphMutationBlock::set_if(
                format!("{} AND eq(len(rk), 0)", parent_gate(observed)),
                quads,
            );
            let lines = vec![revision_lookup("rk", &tenant, &policy.id, sequence)];
            match self.guarded(&scope, "save", observed, lines, block).await? {
                Ok(()) => {
                    let stored = self.revision(account, &policy.id, sequence).await?;
                    return match stored {
                        Some(stored) if stored.digest == revision.digest => Ok(revision),
                        _ => Err(LifecycleError::Unavailable(format!(
                            "revision {sequence} of {} did not land",
                            policy.id
                        ))),
                    };
                }
                Err(error) => {
                    last = error.to_string();
                    tokio::task::yield_now().await;
                }
            }
        }
        Err(LifecycleError::Unavailable(format!(
            "the revision could not be written: {last}"
        )))
    }

    #[allow(clippy::too_many_arguments)]
    async fn activate(
        &self,
        account: &str,
        policy_id: &str,
        sequence: u64,
        expected_head: Option<u64>,
        approver: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let scope = Self::scope(&tenant, policy_id)?;
        let revision = self
            .revision(account, policy_id, sequence)
            .await?
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), sequence))?;
        if !revision.digest_holds() {
            return Err(LifecycleError::CorruptRevision(
                policy_id.to_string(),
                sequence,
            ));
        }

        let mut last = String::new();
        for _ in 0..MOVE_ATTEMPTS {
            let observed = self.observe(&tenant, &scope, policy_id).await?;
            let current = observed.head.as_ref().map(|head| head.sequence);
            if current != expected_head {
                return Err(stale(policy_id, expected_head, current));
            }
            self.separation
                .check_approver(policy_id, &revision.saved_by, approver)?;

            let head = Head {
                policy_id: policy_id.to_string(),
                sequence: revision.sequence,
                digest: revision.digest.clone(),
                activated_by: approver.to_string(),
                editor: revision.saved_by.clone(),
                reason: reason.to_string(),
                activated_at: at,
            };
            let event = activation_event(&revision, expected_head, reason, approver, at);
            let (head_lookup, head_cond, head_node) = head_gate(&tenant, policy_id, expected_head);
            let quads = format!(
                "{}{}",
                head_nquads(&tenant, &head, head_node)?,
                events_nquads(&tenant, observed.guard + 1, &[event])?
            );
            let block = GraphMutationBlock::set_if(
                format!(
                    "{} AND {head_cond} AND eq(len(rv), 1)",
                    parent_gate(observed.guard)
                ),
                quads,
            );
            let lines = vec![
                head_lookup,
                format!(
                    "rv as var(func: eq(hodei.policy.revision.key, {})) \
                     @filter(eq(hodei.policy.revision.digest, {}))",
                    dql_string(&revision_key(&tenant, policy_id, sequence)),
                    dql_string(&revision.digest)
                ),
            ];
            match self
                .guarded(&scope, "activate", observed.guard, lines, block)
                .await?
            {
                Ok(()) => {
                    return self
                        .landed(&tenant, policy_id, sequence, &revision.digest)
                        .await
                }
                Err(error) => {
                    // The guard moved: either another activation (the next
                    // read answers StaleHead) or a save (retry on top).
                    last = error.to_string();
                    tokio::task::yield_now().await;
                }
            }
        }
        Err(LifecycleError::Unavailable(format!(
            "the activation could not be written: {last}"
        )))
    }

    #[allow(clippy::too_many_arguments)]
    async fn rollback(
        &self,
        account: &str,
        policy_id: &str,
        target: u64,
        expected_head: Option<u64>,
        actors: &Actors,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        let editor = actors.editor.as_str();
        let approver = actors.approver.as_str();
        let tenant = Self::tenant(account)?;
        let scope = Self::scope(&tenant, policy_id)?;

        let mut last = String::new();
        for _ in 0..MOVE_ATTEMPTS {
            let observed = self.observe(&tenant, &scope, policy_id).await?;
            let current_head = observed
                .head
                .clone()
                .ok_or_else(|| LifecycleError::NeverActivated(policy_id.to_string()))?;
            let current = current_head.sequence;
            if expected_head != Some(current) {
                return Err(stale(policy_id, expected_head, Some(current)));
            }
            if target == current {
                return Err(LifecycleError::RollbackToHead(
                    policy_id.to_string(),
                    target,
                ));
            }
            let source = self
                .revision(account, policy_id, target)
                .await?
                .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), target))?;
            if !source.digest_holds() {
                return Err(LifecycleError::CorruptRevision(
                    policy_id.to_string(),
                    target,
                ));
            }
            self.separation.check_editor(editor)?;
            self.separation
                .check_approver(policy_id, editor, approver)?;

            let sequence = self.last_saved(&tenant, policy_id).await? + 1;
            if sequence > MAX_REVISIONS {
                return Err(LifecycleError::HistoryFull(policy_id.to_string()));
            }
            let mut policy = source.policy.clone();
            policy.version = format!("{}-rb{sequence}", policy.version);
            let revision = PolicyRevision::new(
                policy,
                sequence,
                editor,
                at,
                format!("{ROLLBACK_PREFIX}{target} {reason}"),
            )
            .rolled_back_by(approver);
            let new_head = Head {
                policy_id: policy_id.to_string(),
                sequence: revision.sequence,
                digest: revision.digest.clone(),
                activated_by: approver.to_string(),
                editor: revision.saved_by.clone(),
                reason: reason.to_string(),
                activated_at: at,
            };
            let events = [
                saved_event(&revision, at),
                activation_event(&revision, Some(current), reason, approver, at),
                invalidation_event(
                    policy_id,
                    current,
                    &current_head.digest,
                    &revision,
                    approver,
                    at,
                ),
            ];
            let (head_lookup, head_cond, head_node) = head_gate(&tenant, policy_id, Some(current));
            let quads = format!(
                "{}{}{}",
                revision_nquads(&tenant, &revision, "_:revision")?,
                head_nquads(&tenant, &new_head, head_node)?,
                events_nquads(&tenant, observed.guard + 1, &events)?
            );
            let block = GraphMutationBlock::set_if(
                format!(
                    "{} AND {head_cond} AND eq(len(rk), 0)",
                    parent_gate(observed.guard)
                ),
                quads,
            );
            let lines = vec![
                head_lookup,
                revision_lookup("rk", &tenant, policy_id, sequence),
            ];
            match self
                .guarded(&scope, "rollback", observed.guard, lines, block)
                .await?
            {
                Ok(()) => {
                    return self
                        .landed(&tenant, policy_id, sequence, &revision.digest)
                        .await
                }
                Err(error) => {
                    last = error.to_string();
                    tokio::task::yield_now().await;
                }
            }
        }
        Err(LifecycleError::Unavailable(format!(
            "the rollback could not be written: {last}"
        )))
    }

    async fn revision(
        &self,
        account: &str,
        policy_id: &str,
        sequence: u64,
    ) -> Result<Option<PolicyRevision>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let query = format!(
            "{{ q(func: eq(hodei.policy.revision.key, {key})) \
             @filter(eq(hodei.policy.revision.tenant, {tenant})) \
             {{ hodei.policy.revision.json }} }}",
            key = dql_string(&revision_key(&tenant, policy_id, sequence)),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        match nodes.len() {
            0 => Ok(None),
            1 => parse_json(&nodes[0], "hodei.policy.revision.json").map(Some),
            n => Err(LifecycleError::Unavailable(format!(
                "{n} revision nodes carry revision {sequence} of {policy_id}"
            ))),
        }
    }

    async fn history(
        &self,
        account: &str,
        policy_id: &str,
    ) -> Result<Vec<PolicyRevision>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let query = format!(
            "{{ q(func: eq(hodei.policy.revision.policy_id, {policy}), \
             orderasc: hodei.policy.revision.sequence) \
             @filter(eq(hodei.policy.revision.tenant, {tenant})) \
             {{ hodei.policy.revision.json }} }}",
            policy = dql_string(policy_id),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        nodes
            .iter()
            .map(|node| parse_json(node, "hodei.policy.revision.json"))
            .collect()
    }

    async fn history_page(
        &self,
        account: &str,
        policy_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<PolicyRevision>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let query = format!(
            "{{ q(func: eq(hodei.policy.revision.policy_id, {policy}), \
             orderasc: hodei.policy.revision.sequence, first: {limit}, offset: {offset}) \
             @filter(eq(hodei.policy.revision.tenant, {tenant})) \
             {{ hodei.policy.revision.json }} }}",
            policy = dql_string(policy_id),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        nodes
            .iter()
            .map(|node| parse_json(node, "hodei.policy.revision.json"))
            .collect()
    }

    async fn events_page(
        &self,
        account: &str,
        policy_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<LifecycleEvent>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let query = format!(
            "{{ q(func: eq(hodei.policy.event.policy_id, {policy}), \
             orderasc: hodei.policy.event.order, first: {limit}, offset: {offset}) \
             @filter(eq(hodei.policy.event.tenant, {tenant})) \
             {{ hodei.policy.event.json }} }}",
            policy = dql_string(policy_id),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        nodes
            .iter()
            .map(|node| parse_json(node, "hodei.policy.event.json"))
            .collect()
    }

    async fn head(&self, account: &str, policy_id: &str) -> Result<Option<Head>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        self.read_head(&tenant, policy_id).await
    }

    async fn events(
        &self,
        account: &str,
        policy_id: &str,
    ) -> Result<Vec<LifecycleEvent>, LifecycleError> {
        let tenant = Self::tenant(account)?;
        let query = format!(
            "{{ q(func: eq(hodei.policy.event.policy_id, {policy}), \
             orderasc: hodei.policy.event.order) \
             @filter(eq(hodei.policy.event.tenant, {tenant})) \
             {{ hodei.policy.event.json }} }}",
            policy = dql_string(policy_id),
            tenant = dql_string(tenant.as_str()),
        );
        let nodes = rows(
            &query_json(&self.client, &query)
                .await
                .map_err(unavailable)?,
            "q",
        );
        nodes
            .iter()
            .map(|node| parse_json(node, "hodei.policy.event.json"))
            .collect()
    }
}

impl DgraphLifecycleStore {
    /// The head after a committed move, read back: success is decided by
    /// the state, never inferred from the transaction having committed.
    async fn landed(
        &self,
        tenant: &Tenant,
        policy_id: &str,
        sequence: u64,
        digest: &str,
    ) -> Result<Head, LifecycleError> {
        match self.read_head(tenant, policy_id).await? {
            Some(head) if head.sequence == sequence && head.digest == digest => Ok(head),
            other => Err(LifecycleError::Unavailable(format!(
                "the head of {policy_id} is {:?} after the commit, expected revision {sequence}",
                other.map(|head| head.sequence)
            ))),
        }
    }
}
