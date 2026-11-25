//! The projection frontier: the boundary where the graph changeset, its
//! deltas and its checkpoint commit as ONE fact.
//!
//! The old frontier was two writes: the changeset went into the graph,
//! and afterwards — a separate round trip — the checkpoint said the
//! event was seen. A crash in between left the mutation applied and the
//! checkpoint behind, and the replay could not tell "applied" from "not
//! applied" without guessing. This adapter closes that window with the
//! primitives the stack already proves:
//!
//! - **One transaction.** The guarded write of [`hodei_graph_client::guard`]
//!   carries the graph changeset (the HSP-189 upsert, extended with the
//!   projection name), the delta records and the checkpoint advance in
//!   the SAME upsert as the guard advance and the operation marker. The
//!   commit is the frontier: either the graph changed, the deltas were
//!   said and the checkpoint moved, or none of it happened.
//! - **The delivery is an operation.** The operation id is derived from
//!   (tenant, consumer, stream, position, event id), so the same
//!   delivery re-driven after a lost ACK finds its marker and returns
//!   the committed receipt without writing anything — the same
//!   once-only contract the ledger appends run under.
//! - **The writers of one checkpoint serialize.** The guard scope
//!   (`checkpoint|tenant|consumer/stream`) is ONE node whose revision
//!   every writer advances inside its own transaction; two concurrent
//!   writers meet on the `@upsert` token and the server decides. A
//!   stale writer writes NOTHING (every block is gated on the
//!   in-transaction revision) and retries on top of whatever committed.
//! - **The checkpoint never regresses.** The update branch is gated on
//!   the in-transaction position being BEHIND the claimed one, and the
//!   create branch on the row being absent: a re-delivery of an event
//!   the checkpoint already passed moves nothing.
//! - **The deltas are once-only twice over.** Each carries a dedup key
//!   (`tenant#cause#kind#relation#evidence`), exact and `@upsert`,
//!   created only when absent; the same change said twice — by a replay
//!   or by another consumer — is ONE record. Their sequence is the
//!   deterministic per-delta order of the delivery (`position+1+i` over
//!   the sorted dedup keys), so a replay recomputes the same version it
//!   first wrote.
//! - **The receipt.** The operation marker records what certifies the
//!   delta of this delivery was applied: the ledger position it came
//!   from, the next position the checkpoint claimed, and the
//!   deterministic changeset fingerprint of HSP-189. A replay returns
//!   exactly that text.
//!
//! # What the frontier does not decide
//!
//! The projection stays a projection: the deltas are facts of the ACTIVE
//! graph, and a rebuild that replays the ledger into its own named
//! projection records its watermark through the same frontier WITHOUT
//! emitting deltas (the deltas were said when the active projection
//! applied the event the first time).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use hodei_evidence::application::delta::{SupportSnapshot, relation_deltas};
use hodei_evidence::application::projection::{
    ProjectionDelivery, ProjectionFrontier, ProjectionReceipt,
};
use hodei_evidence::domain::delta::RelationDelta;
use hodei_evidence::domain::graph::GraphChangeSet;
use hodei_evidence::ports::graph::GraphError;
use hodei_graph_client::{
    GraphClient, GraphMutationBlock, GuardError, GuardKind, GuardScope, GuardStore, GuardedEffect,
    OperationId, RevisionDemand, Tenant, dql_string,
};
use kernel::Hrn;
use sha2::{Digest, Sha256};

use super::graph::{ACTIVE_PROJECTION, DgraphGraphStore};

/// The type tag of checkpoint nodes in the graph.
pub const CHECKPOINT_TYPE: &str = "HodeiCheckpoint";
/// The type tag of delta nodes in the graph.
pub const DELTA_TYPE: &str = "HodeiDelta";

/// Why the frontier could not do its job. Every variant is closed: the
/// caller knows whether the write was refused, whether it lost every
/// retry, or whether the graph could not be reached at all.
#[derive(Debug, thiserror::Error)]
pub enum FrontierError {
    /// The store could not open on a validated schema.
    #[error("the frontier could not open on the graph: {0}")]
    Setup(Box<GuardError>),
    /// The delivery names a consumer or stream that cannot be a
    /// conflict key.
    #[error("the delivery is not addressable: {0}")]
    InvalidDelivery(String),
    /// The write did not commit after its bounded retries, or the
    /// delivery contradicts the contract.
    #[error("the frontier write did not commit: {0}")]
    Rejected(String),
    /// The graph could not be reached, or its answer could not be read.
    #[error("the frontier could not reach the graph: {0}")]
    Unavailable(String),
}

impl From<GuardError> for FrontierError {
    fn from(error: GuardError) -> Self {
        match error {
            error @ GuardError::Schema(_) => FrontierError::Setup(Box::new(error)),
            GuardError::Graph(_) | GuardError::Operation(_) => {
                FrontierError::Unavailable(error.to_string())
            }
            GuardError::InvalidScope { scope, reason } => {
                FrontierError::InvalidDelivery(format!("{scope}: {reason}"))
            }
            GuardError::CasRejected { .. }
            | GuardError::CorruptState { .. }
            | GuardError::RetryExhausted { .. } => FrontierError::Rejected(error.to_string()),
        }
    }
}

impl From<FrontierError> for GraphError {
    fn from(error: FrontierError) -> Self {
        match error {
            FrontierError::Unavailable(_) => GraphError::Unavailable(error.to_string()),
            FrontierError::Setup(_) | FrontierError::InvalidDelivery(_) => {
                GraphError::Rejected(error.to_string())
            }
            FrontierError::Rejected(_) => GraphError::Rejected(error.to_string()),
        }
    }
}

/// The answer of a frontier commit: whether it replayed, where the
/// checkpoint stands, and the receipt that certifies the delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierCommit {
    /// The scope key the commit ran under.
    pub scope: String,
    /// The operation id of the delivery.
    pub operation_id: String,
    /// True when the delivery had already committed: nothing was
    /// written again, and the receipt is the committed one.
    pub replayed: bool,
    /// The checkpoint the store reports after the commit.
    pub checkpoint: u64,
    /// The receipt: `pos=<position>;next=<next>;cs=<fingerprint>`.
    pub receipt: String,
    /// The revision of the checkpoint guard after the commit.
    pub guard_revision: u64,
}

/// The frontier of one projection, over the graph authority.
///
/// Built on a connected, logged-in client; [`DgraphProjectionFrontier::open`]
/// runs the schema boot like every adapter of this crate. The frontier
/// of the ACTIVE projection emits deltas; a named one records watermarks
/// only.
#[derive(Debug)]
pub struct DgraphProjectionFrontier {
    guards: GuardStore,
    client: Arc<GraphClient>,
    tenant: Tenant,
    projection: String,
}

impl DgraphProjectionFrontier {
    /// Opens the frontier of the active projection for one tenant
    /// namespace.
    pub async fn open(client: Arc<GraphClient>, tenant: &str) -> Result<Self, FrontierError> {
        Self::named(client, tenant, ACTIVE_PROJECTION).await
    }

    /// Opens the frontier of a named projection: the rebuild records its
    /// watermark here and never emits deltas.
    pub async fn named(
        client: Arc<GraphClient>,
        tenant: &str,
        projection: &str,
    ) -> Result<Self, FrontierError> {
        let tenant = Tenant::new(tenant).map_err(|error| {
            FrontierError::InvalidDelivery(format!("the tenant '{tenant}' is not valid: {error}"))
        })?;
        let guards = GuardStore::open(client.clone()).await?;
        Ok(Self {
            guards,
            client,
            tenant,
            projection: projection.to_owned(),
        })
    }

    /// The identity of one checkpoint: the tenant namespace, the
    /// consumer and the stream. Deterministic, so a restart looks the
    /// checkpoint up instead of finding two.
    pub fn checkpoint_key(tenant: &str, consumer: &str, stream_id: &str) -> String {
        sha256_key(&format!("{tenant}#{consumer}#{stream_id}"))
    }

    /// The identity of one delivery: tenant, consumer, stream, position
    /// and event id. A re-delivery of the same envelope carries the same
    /// id and replays its committed receipt.
    pub fn delivery_operation_id(
        tenant: &str,
        consumer: &str,
        stream_id: &str,
        position: u64,
        event_id: &str,
    ) -> OperationId {
        let digest = Sha256::digest(
            format!("{tenant}#{consumer}#{stream_id}#{position}#{event_id}").as_bytes(),
        );
        OperationId::new(&hex::encode(digest)).expect("a sha256 hex is always a valid id")
    }

    /// The dedup identity of one delta: the tenant, the cause, what the
    /// delta says and the evidence that moved it. The same change said
    /// twice is ONE record.
    pub fn delta_key(tenant: &str, delta: &RelationDelta) -> String {
        sha256_key(&format!(
            "{tenant}#{}#{}#{}#{}",
            delta.cause,
            delta.kind.as_str(),
            delta.identity(),
            delta.evidence.as_str()
        ))
    }

    /// The text a commit records on its marker: what certifies the delta
    /// of this delivery was applied — the ledger position it came from,
    /// the next position the checkpoint claimed, and the deterministic
    /// fingerprint of the changeset that carried it.
    pub fn receipt_text(changes: &GraphChangeSet, next: u64) -> String {
        format!(
            "pos={};next={next};cs={}",
            next.saturating_sub(1),
            DgraphGraphStore::changeset_fingerprint(changes)
        )
    }

    /// The scope that serializes the writers of one checkpoint.
    fn scope(&self, delivery: &ProjectionDelivery) -> Result<GuardScope, FrontierError> {
        let subject = format!("{}/{}", delivery.consumer, delivery.stream_id);
        GuardScope::new(GuardKind::Checkpoint, &self.tenant, &subject)
            .map_err(|error| FrontierError::InvalidDelivery(error.to_string()))
    }

    /// Reads one checkpoint row. `None` when the consumer has not
    /// started: the absence is a response, not an error.
    pub async fn checkpoint_row(
        &self,
        consumer: &str,
        stream_id: &str,
    ) -> Result<Option<CheckpointRow>, FrontierError> {
        let key = Self::checkpoint_key(self.tenant.as_str(), consumer, stream_id);
        let query = format!(
            "{{ q(func: eq(hodei.checkpoint.key, {})) {{ {} }} }}",
            dql_string(&key),
            CHECKPOINT_FIELDS
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| FrontierError::Unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => CheckpointRow::from_node(&nodes[0]).map(Some),
            n => Err(FrontierError::Rejected(format!(
                "{n} checkpoint nodes carry the key '{key}'"
            ))),
        }
    }

    /// Where a consumer is: the next position to project, or nothing
    /// when it has not started.
    pub async fn checkpoint_position(
        &self,
        consumer: &str,
        stream_id: &str,
    ) -> Result<Option<u64>, FrontierError> {
        Ok(self
            .checkpoint_row(consumer, stream_id)
            .await?
            .map(|row| row.next_position))
    }

    /// The version the tenant's delta story has reached: the biggest
    /// sequence the tenant holds, or zero when nothing was said.
    pub async fn delta_version(&self, tenant: &Hrn) -> Result<u64, FrontierError> {
        let query = format!(
            "{{ q(func: eq(hodei.delta.tenant, {})) {{ hodei.delta.seq }} }}",
            dql_string(&tenant.to_string())
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| FrontierError::Unavailable(error.to_string()))?;
        Ok(response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|node| {
                node.get("hodei.delta.seq")
                    .and_then(serde_json::Value::as_u64)
            })
            .max()
            .unwrap_or(0))
    }

    /// The deltas a subscriber wants, after a version, within a quota.
    /// The quota keeps ties whole: when the page ends inside a group of
    /// deltas that share a sequence, the reader is sent back before the
    /// group, so the next read takes the whole group again — a delta is
    /// delivered twice before it is lost, never the other way around.
    pub async fn pending_deltas(
        &self,
        subscription: &hodei_evidence::domain::delta::RelationDeltaSubscription,
        after_version: u64,
        quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), FrontierError> {
        if subscription.relations.is_empty() || quota == 0 {
            return Ok((Vec::new(), after_version));
        }
        let types: Vec<String> = subscription
            .relations
            .iter()
            .map(|relation| relation.as_str().to_string())
            .collect();
        let query = format!(
            "{{ q(func: eq(hodei.delta.tenant, {tenant})) \
             @filter(gt(hodei.delta.seq, {after})) {{ {DELTA_FIELDS} }} }}",
            tenant = dql_string(&subscription.tenant.to_string()),
            after = after_version,
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| FrontierError::Unavailable(error.to_string()))?;
        let mut rows: Vec<DeltaRow> = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|node| DeltaRow::from_node(node).ok())
            .filter(|row| types.contains(&row.raw.relation))
            .collect();
        rows.sort_by_key(|row| row.seq);

        let mut wanted = Vec::new();
        let mut stopped = after_version;
        for (index, row) in rows.iter().enumerate() {
            if index >= quota {
                // The page is full: if the first excluded row ties the
                // last included one, the reader is sent back before the
                // tie, so the group is re-read whole, never half.
                if let (Some(last), Some(next)) = (rows.get(quota - 1), rows.get(quota))
                    && next.seq == last.seq
                {
                    stopped = stopped.max(next.seq.saturating_sub(1));
                }
                break;
            }
            wanted.push((row.seq, row.delta()));
            stopped = stopped.max(row.seq);
        }
        Ok((wanted, stopped))
    }

    /// The deltas of EVERY tenant, ordered by tenant and then by
    /// version, at most `quota` of them. It serves the reconciler of the
    /// whole store, which does not know the tenants beforehand. A stored
    /// row that cannot be read back whole is skipped: the reconciliation
    /// converges, and one corrupt row must not stop the rest.
    pub async fn all_deltas(&self, quota: usize) -> Result<Vec<RelationDelta>, FrontierError> {
        if quota == 0 {
            return Ok(Vec::new());
        }
        let query = format!(
            "{{ q(func: type({DELTA_TYPE}), orderasc: hodei.delta.tenant, \
             orderasc: hodei.delta.seq, first: {quota}) {{ {DELTA_FIELDS} }} }}"
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| FrontierError::Unavailable(error.to_string()))?;
        let mut rows: Vec<DeltaRow> = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|node| DeltaRow::from_node(node).ok())
            .collect();
        rows.sort_by(|a, b| (&a.raw.tenant, a.seq).cmp(&(&b.raw.tenant, b.seq)));
        Ok(rows.iter().filter_map(DeltaRow::try_delta).collect())
    }

    /// Applies the changeset and moves the checkpoint as ONE commit.
    /// `emit_deltas` is false for a rebuild watermark: the rebuild
    /// records how far it got and says nothing — the deltas of the event
    /// were said when the active projection applied it.
    pub async fn apply_and_checkpoint_with(
        &self,
        changes: &GraphChangeSet,
        delivery: &ProjectionDelivery,
        emit_deltas: bool,
    ) -> Result<FrontierCommit, FrontierError> {
        let scope = self.scope(delivery)?;
        let operation = Self::delivery_operation_id(
            self.tenant.as_str(),
            &delivery.consumer,
            &delivery.stream_id,
            delivery.position,
            &delivery.event_id,
        );
        let next = delivery.position.saturating_add(1);
        let fingerprint = DgraphGraphStore::changeset_fingerprint(changes);
        let receipt = Self::receipt_text(changes, next);
        let checkpoint_key = Self::checkpoint_key(
            self.tenant.as_str(),
            &delivery.consumer,
            &delivery.stream_id,
        );

        // The deltas are computed BEFORE the transaction, from the
        // supports the graph holds today — the same pure function the
        // Surreal emitter runs. The write is gated on each delta's key
        // being absent, so a recomputation that lands after the first
        // commit says nothing twice.
        let deltas = self
            .the_deltas_to_say(changes, delivery, emit_deltas)
            .await?;

        // The changeset plan: its bindings ride in the guarded query and
        // its blocks ride in the guarded transaction, each gated on the
        // operation being fresh and the in-transaction revision matching
        // the pre-read one — a stale writer writes nothing and retries.
        let changeset_tx = DgraphGraphStore::changeset_upsert_scoped(
            changes,
            &self.graph_prefix(),
            &self.projection,
        );
        let changeset_body = query_body(&changeset_tx.query).to_owned();
        let changeset_blocks = changeset_tx.blocks;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let effect = GuardedEffect {
            query_lines: {
                let mut lines = Vec::new();
                if !changeset_body.is_empty() {
                    lines.push(changeset_body.clone());
                }
                lines.extend(checkpoint_query_lines(&checkpoint_key, next));
                lines.extend(delta_query_lines(&deltas));
                lines
            },
            set_nquads: String::new(),
            recorded_result: receipt.clone(),
        };

        let outcome = self
            .guards
            .execute_guarded_composite(
                &scope,
                &operation,
                RevisionDemand::Advance,
                effect,
                &|current| {
                    let parent = parent_gate(current);
                    let mut blocks: Vec<GraphMutationBlock> = changeset_blocks
                        .iter()
                        .map(|block| gate_block(block, parent))
                        .collect();
                    blocks.push(GraphMutationBlock::set_if(
                        format!("{parent} AND eq(len(ck), 0)"),
                        checkpoint_create_nquads(
                            &checkpoint_key,
                            self.tenant.as_str(),
                            &delivery.consumer,
                            &delivery.stream_id,
                            next,
                            &delivery.event_id,
                            &fingerprint,
                            &now,
                        ),
                    ));
                    blocks.push(GraphMutationBlock::set_if(
                        format!("{parent} AND eq(len(ckb), 1)"),
                        checkpoint_update_nquads(next, &delivery.event_id, &fingerprint, &now),
                    ));
                    for delta in &deltas {
                        blocks.push(GraphMutationBlock::set_if(
                            format!("{parent} AND eq(len({}), 0)", delta.binding),
                            delta_create_nquads(delta, &now),
                        ));
                    }
                    blocks
                },
            )
            .await?;

        // The state after the commit answers, never the hope: the
        // checkpoint is read back from the store.
        let checkpoint = self
            .checkpoint_position(&delivery.consumer, &delivery.stream_id)
            .await?
            .unwrap_or(0);
        Ok(FrontierCommit {
            scope: outcome.scope,
            operation_id: outcome.operation_id,
            replayed: outcome.replayed,
            checkpoint,
            receipt,
            guard_revision: outcome.guard_revision,
        })
    }

    /// The graph key prefix of the projection this frontier serves: the
    /// active projection keeps the bare keys, a named one namespaces its
    /// own — the same rule the graph store runs.
    fn graph_prefix(&self) -> String {
        if self.projection == ACTIVE_PROJECTION {
            String::new()
        } else {
            format!("{}|", self.projection)
        }
    }

    /// The deltas the delivery says, in their deterministic order, with
    /// the bindings their blocks will be gated on. Empty when the
    /// changeset carries no cause (nothing caused it, nothing is said)
    /// or when the caller asked for watermarks only.
    async fn the_deltas_to_say(
        &self,
        changes: &GraphChangeSet,
        delivery: &ProjectionDelivery,
        emit_deltas: bool,
    ) -> Result<Vec<PlannedDelta>, FrontierError> {
        let Some(cause) = changes.cause.clone() else {
            return Ok(Vec::new());
        };
        if !emit_deltas || (changes.relations.is_empty() && changes.withdrawals.is_empty()) {
            return Ok(Vec::new());
        }
        let before = self.the_supports_before(changes).await?;
        let said = relation_deltas(&cause, changes, &before, Utc::now());
        // The order is the sorted dedup keys: a replay recomputes the
        // same order and therefore the same version per delta.
        let mut keyed: BTreeMap<String, RelationDelta> = BTreeMap::new();
        for delta in said {
            keyed.insert(Self::delta_key(self.tenant.as_str(), &delta), delta);
        }
        Ok(keyed
            .into_iter()
            .enumerate()
            .map(|(index, (key, delta))| PlannedDelta {
                binding: format!("dk{index}"),
                key,
                seq: delivery.position.saturating_add(1 + index as u64),
                delta,
            })
            .collect())
    }

    /// The supports each touched relation held BEFORE the changeset: the
    /// snapshot the delta emitter compares against. The read is a hint
    /// taken before the transaction; the write is dedup'd by key, so a
    /// snapshot overtaken by a concurrent commit costs a retry, never a
    /// duplicated fact.
    async fn the_supports_before(
        &self,
        changes: &GraphChangeSet,
    ) -> Result<SupportSnapshot, FrontierError> {
        let mut identities: Vec<(String, &Hrn)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for relation in &changes.relations {
            if seen.insert(relation.identity()) {
                identities.push((relation.identity(), &relation.tenant_hrn));
            }
        }
        for withdrawal in &changes.withdrawals {
            if seen.insert(withdrawal.identity()) {
                identities.push((withdrawal.identity(), &withdrawal.tenant_hrn));
            }
        }
        if identities.is_empty() {
            return Ok(SupportSnapshot::new());
        }

        let mut query = String::from("{ ");
        for (index, (identity, tenant)) in identities.iter().enumerate() {
            let key = format!(
                "{}{}",
                self.graph_prefix(),
                DgraphGraphStore::relation_key(&tenant.to_string(), identity)
            );
            query.push_str(&format!(
                "q{index}(func: eq(hodei.graph.rel.key, {})) {{ \
                 hodei.graph.supports {{ hodei.graph.support.evidence }} }}\n",
                dql_string(&key)
            ));
        }
        query.push('}');
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| FrontierError::Unavailable(error.to_string()))?;

        let mut snapshot = SupportSnapshot::new();
        for (index, (identity, _tenant)) in identities.iter().enumerate() {
            let supports = response
                .json
                .get(format!("q{index}"))
                .and_then(|q| q.get(0))
                .and_then(|node| node.get("hodei.graph.supports"))
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|support| {
                    support
                        .get("hodei.graph.support.evidence")
                        .and_then(serde_json::Value::as_str)
                        .map(hodei_evidence::EvidenceId::new)
                })
                .collect::<Vec<_>>();
            if !supports.is_empty() {
                snapshot.insert(identity.clone(), supports);
            }
        }
        Ok(snapshot)
    }
}

/// One delta the transaction will say, with the binding its gate names
/// and the sequence it takes in the tenant's story.
#[derive(Debug, Clone)]
struct PlannedDelta {
    /// The query variable the delta's key binds on (`dk0`).
    binding: String,
    /// The dedup key of the delta.
    key: String,
    /// The version the delta takes: the delivery's position, plus the
    /// delta's place in the deterministic order.
    seq: u64,
    /// The delta itself.
    delta: RelationDelta,
}

/// The fields every checkpoint row is read with.
const CHECKPOINT_FIELDS: &str = "hodei.checkpoint.key hodei.checkpoint.consumer \
     hodei.checkpoint.stream_id hodei.checkpoint.tenant hodei.checkpoint.next_position \
     hodei.checkpoint.last_event hodei.checkpoint.last_fingerprint hodei.checkpoint.updated_at";

/// The fields every delta row is read with.
const DELTA_FIELDS: &str = "hodei.delta.key hodei.delta.seq hodei.delta.tenant \
     hodei.delta.kind hodei.delta.relation hodei.delta.from hodei.delta.to \
     hodei.delta.evidence hodei.delta.cause hodei.delta.observed_at";

/// One row of the checkpoints, as it is stored: the consumer, the
/// stream, and the next position to project — the same semantic the
/// Surreal checkpoint row carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRow {
    /// The consumer that projects.
    pub consumer: String,
    /// The stream it projects.
    pub stream_id: String,
    /// The next position it has to project.
    pub next_position: u64,
    /// The event the checkpoint last certified.
    pub last_event: String,
    /// The fingerprint of the changeset that was applied last.
    pub last_fingerprint: String,
}

impl CheckpointRow {
    /// Reads one stored node. A missing field is corruption, reported —
    /// never a default that would hide a hole.
    fn from_node(node: &serde_json::Value) -> Result<Self, FrontierError> {
        let text = |field: &str| -> Result<String, FrontierError> {
            node.get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    FrontierError::Rejected(format!("the checkpoint node has no string '{field}'"))
                })
        };
        Ok(Self {
            consumer: text("hodei.checkpoint.consumer")?,
            stream_id: text("hodei.checkpoint.stream_id")?,
            next_position: node
                .get("hodei.checkpoint.next_position")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    FrontierError::Rejected("the checkpoint node has no next position".to_owned())
                })?,
            last_event: node
                .get("hodei.checkpoint.last_event")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            last_fingerprint: node
                .get("hodei.checkpoint.last_fingerprint")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }
}

/// One stored delta row, read back for the consumers that invalidate.
#[derive(Debug, Clone)]
struct DeltaRow {
    /// The version the delta takes in the tenant's story.
    seq: u64,
    /// The stored fields the delta rebuilds from.
    raw: DeltaRaw,
}

/// The stored fields a delta rebuilds from.
#[derive(Debug, Clone)]
struct DeltaRaw {
    tenant: String,
    kind: String,
    relation: String,
    from: String,
    to: String,
    evidence: String,
    cause: String,
    observed_at: String,
}

impl DeltaRow {
    /// Reads one stored node. A row that cannot be read back is skipped
    /// by the scan (and reported through the error of nothing): the
    /// deltas a consumer reads are the ones the frontier wrote whole.
    fn from_node(node: &serde_json::Value) -> Result<Self, FrontierError> {
        let text = |field: &str| -> Option<String> {
            node.get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let raw = DeltaRaw {
            tenant: text("hodei.delta.tenant").ok_or_else(|| {
                FrontierError::Rejected("the delta node has no tenant".to_owned())
            })?,
            kind: text("hodei.delta.kind")
                .ok_or_else(|| FrontierError::Rejected("the delta node has no kind".to_owned()))?,
            relation: text("hodei.delta.relation").ok_or_else(|| {
                FrontierError::Rejected("the delta node has no relation".to_owned())
            })?,
            from: text("hodei.delta.from")
                .ok_or_else(|| FrontierError::Rejected("the delta node has no from".to_owned()))?,
            to: text("hodei.delta.to")
                .ok_or_else(|| FrontierError::Rejected("the delta node has no to".to_owned()))?,
            evidence: text("hodei.delta.evidence").ok_or_else(|| {
                FrontierError::Rejected("the delta node has no evidence".to_owned())
            })?,
            cause: text("hodei.delta.cause")
                .ok_or_else(|| FrontierError::Rejected("the delta node has no cause".to_owned()))?,
            observed_at: text("hodei.delta.observed_at").unwrap_or_default(),
        };
        Ok(Self {
            seq: node
                .get("hodei.delta.seq")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    FrontierError::Rejected("the delta node has no sequence".to_owned())
                })?,
            raw,
        })
    }

    /// The domain delta the row says, or `None` when a stored field is
    /// not the identifier it must be. The scan of the whole store reads
    /// through this one: a row nobody can parse is not a panic.
    fn try_delta(&self) -> Option<RelationDelta> {
        Hrn::from_string(&self.raw.tenant)?;
        Hrn::from_string(&self.raw.from)?;
        Hrn::from_string(&self.raw.to)?;
        hodei_evidence::RelationType::parse(&self.raw.relation)?;
        Some(self.delta())
    }

    /// The domain delta the row says.
    fn delta(&self) -> RelationDelta {
        let kind = match self.raw.kind.as_str() {
            "added" => hodei_evidence::domain::delta::RelationDeltaKind::Added,
            "removed" => hodei_evidence::domain::delta::RelationDeltaKind::Removed,
            _ => hodei_evidence::domain::delta::RelationDeltaKind::SupportChanged,
        };
        RelationDelta {
            kind,
            tenant: Hrn::from_string(&self.raw.tenant)
                .expect("the stored tenant of a delta is an HRN"),
            relation: hodei_evidence::RelationType::parse(&self.raw.relation)
                .expect("the stored relation of a delta is a type"),
            from: Hrn::from_string(&self.raw.from).expect("the stored from is an HRN"),
            to: Hrn::from_string(&self.raw.to).expect("the stored to is an HRN"),
            evidence: hodei_evidence::EvidenceId::new(self.raw.evidence.clone()),
            cause: self.raw.cause.clone(),
            observed_at: chrono::DateTime::parse_from_rfc3339(&self.raw.observed_at)
                .map(|instant| instant.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

/// The parent conjunct every frontier block is gated on, derived from
/// the revision the attempt's pre-read observed: a fresh scope runs the
/// create branch (the guard binds as `g`), an existing one the advance
/// branch (the guard binds as `gv`, filtered by the expected revision).
fn parent_gate(current: u64) -> &'static str {
    if current == 0 {
        "eq(len(op), 0) AND eq(len(g), 0)"
    } else {
        "eq(len(op), 0) AND eq(len(gv), 1)"
    }
}

/// Gates one changeset block with the parent conjunct: the changeset is
/// part of the frontier fact, and a stale writer writes none of it.
fn gate_block(block: &GraphMutationBlock, parent: &str) -> GraphMutationBlock {
    let cond = match &block.cond {
        Some(own) => format!("{parent} AND {own}"),
        None => parent.to_owned(),
    };
    GraphMutationBlock {
        cond: Some(cond),
        set_nquads: block.set_nquads.clone(),
        delete_nquads: block.delete_nquads.clone(),
    }
}

/// The query lines that bind the checkpoint row: the unfiltered lookup
/// the create branch gates on, and the BEHIND-filtered one the update
/// branch gates on. An update whose row is already at or past the
/// claimed position binds nothing and writes nothing — the checkpoint
/// never regresses.
fn checkpoint_query_lines(key: &str, next: u64) -> Vec<String> {
    vec![
        format!(
            "ck as var(func: eq(hodei.checkpoint.key, {}))",
            dql_string(key)
        ),
        format!(
            "ckb as var(func: eq(hodei.checkpoint.key, {})) \
             @filter(lt(hodei.checkpoint.next_position, {next}))",
            dql_string(key)
        ),
    ]
}

/// The query lines that bind the delta rows the write may create.
fn delta_query_lines(deltas: &[PlannedDelta]) -> Vec<String> {
    deltas
        .iter()
        .map(|delta| {
            format!(
                "{} as var(func: eq(hodei.delta.key, {}))",
                delta.binding,
                dql_string(&delta.key)
            )
        })
        .collect()
}

/// The N-Quads that create one checkpoint row.
#[allow(clippy::too_many_arguments)]
fn checkpoint_create_nquads(
    key: &str,
    tenant: &str,
    consumer: &str,
    stream_id: &str,
    next: u64,
    event_id: &str,
    fingerprint: &str,
    now: &str,
) -> String {
    format!(
        "_:ck <dgraph.type> \"{CHECKPOINT_TYPE}\" .\n\
         _:ck <hodei.checkpoint.key> {} .\n\
         _:ck <hodei.checkpoint.consumer> {} .\n\
         _:ck <hodei.checkpoint.stream_id> {} .\n\
         _:ck <hodei.checkpoint.tenant> {} .\n\
         _:ck <hodei.checkpoint.next_position> \"{next}\" .\n\
         _:ck <hodei.checkpoint.last_event> {} .\n\
         _:ck <hodei.checkpoint.last_fingerprint> {} .\n\
         _:ck <hodei.checkpoint.updated_at> {} .\n",
        dql_string(key),
        dql_string(consumer),
        dql_string(stream_id),
        dql_string(tenant),
        dql_string(event_id),
        dql_string(fingerprint),
        dql_string(now),
    )
}

/// The N-Quads that move one checkpoint row forward, by the uid the
/// BEHIND filter bound.
fn checkpoint_update_nquads(next: u64, event_id: &str, fingerprint: &str, now: &str) -> String {
    format!(
        "uid(ckb) <hodei.checkpoint.next_position> \"{next}\" .\n\
         uid(ckb) <hodei.checkpoint.last_event> {} .\n\
         uid(ckb) <hodei.checkpoint.last_fingerprint> {} .\n\
         uid(ckb) <hodei.checkpoint.updated_at> {} .\n",
        dql_string(event_id),
        dql_string(fingerprint),
        dql_string(now),
    )
}

/// The N-Quads that create one delta record. The tenant the row
/// carries is the DOMAIN tenant of the relation (the one the
/// subscription scans by), never the adapter's namespace.
fn delta_create_nquads(delta: &PlannedDelta, now: &str) -> String {
    let binding = &delta.binding;
    format!(
        "_:{binding} <dgraph.type> \"{DELTA_TYPE}\" .\n\
         _:{binding} <hodei.delta.key> {key} .\n\
         _:{binding} <hodei.delta.seq> \"{seq}\" .\n\
         _:{binding} <hodei.delta.tenant> {tenant_lit} .\n\
         _:{binding} <hodei.delta.kind> {kind} .\n\
         _:{binding} <hodei.delta.relation> {relation} .\n\
         _:{binding} <hodei.delta.from> {from} .\n\
         _:{binding} <hodei.delta.to> {to} .\n\
         _:{binding} <hodei.delta.evidence> {evidence} .\n\
         _:{binding} <hodei.delta.cause> {cause} .\n\
         _:{binding} <hodei.delta.observed_at> {observed} .\n",
        key = dql_string(&delta.key),
        seq = delta.seq,
        tenant_lit = dql_string(&delta.delta.tenant.to_string()),
        kind = dql_string(delta.delta.kind.as_str()),
        relation = dql_string(delta.delta.relation.as_str()),
        from = dql_string(&delta.delta.from.to_string()),
        to = dql_string(&delta.delta.to.to_string()),
        evidence = dql_string(delta.delta.evidence.as_str()),
        cause = dql_string(&delta.delta.cause),
        observed = dql_string(now),
    )
}

/// The body of an upsert query block, without the enclosing braces.
fn query_body(query: &str) -> &str {
    let body = query.strip_prefix("{\n").unwrap_or(query);
    body.strip_suffix("}\n").unwrap_or(body)
}

fn sha256_key(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

#[async_trait]
impl ProjectionFrontier for DgraphProjectionFrontier {
    async fn apply_and_checkpoint(
        &self,
        changes: GraphChangeSet,
        delivery: &ProjectionDelivery,
    ) -> Result<ProjectionReceipt, GraphError> {
        let commit = self
            .apply_and_checkpoint_with(&changes, delivery, true)
            .await?;
        Ok(ProjectionReceipt {
            replayed: commit.replayed,
            checkpoint: commit.checkpoint,
            receipt: commit.receipt,
        })
    }

    async fn checkpoint(&self, consumer: &str, stream_id: &str) -> Result<Option<u64>, GraphError> {
        self.checkpoint_position(consumer, stream_id)
            .await
            .map_err(FrontierError::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::domain::graph::{GraphRelation, NodeKind, RelationType};
    use hodei_evidence::{EvidenceId, GraphNode};

    fn tenant() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("an HRN")
    }

    fn hrn(kind: &str, id: &str) -> Hrn {
        Hrn::from_string(&format!("hrn:hodei:artifacts:eu-west-1:acme:{kind}/{id}"))
            .expect("an HRN")
    }

    fn changeset() -> GraphChangeSet {
        let now = Utc::now();
        GraphChangeSet::empty()
            .with_cause("evt-1")
            .with_node(GraphNode::new(
                hrn("Artifact", "app"),
                tenant(),
                NodeKind::Artifact,
                now,
            ))
            .with_node(GraphNode::new(
                hrn("Component", "lib"),
                tenant(),
                NodeKind::Component,
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                hrn("Artifact", "app"),
                hrn("Component", "lib"),
                tenant(),
                EvidenceId::new("evd-1"),
                now,
            ))
    }

    /// The checkpoint key is derived, deterministic and tenant-scoped:
    /// a restart looks the same row up, two tenants never share one.
    #[test]
    fn the_checkpoint_key_is_deterministic_and_scoped() {
        let first =
            DgraphProjectionFrontier::checkpoint_key("acme", "graph-projector", "tenant-acme");
        let same =
            DgraphProjectionFrontier::checkpoint_key("acme", "graph-projector", "tenant-acme");
        assert_eq!(first, same, "the key is derived, not generated");
        assert_ne!(
            first,
            DgraphProjectionFrontier::checkpoint_key("other", "graph-projector", "tenant-acme"),
            "two tenants never share a checkpoint"
        );
        assert_ne!(
            first,
            DgraphProjectionFrontier::checkpoint_key("acme", "risk", "tenant-acme"),
            "two consumers never share a checkpoint"
        );
        assert!(!first.contains('|'), "the key survives the DQL literal");
    }

    /// The operation id IS the delivery: the same four delivered again
    /// are the same operation; anything else is another one.
    #[test]
    fn the_operation_id_is_the_delivery() {
        let a = DgraphProjectionFrontier::delivery_operation_id("acme", "graph", "s", 3, "evt-1");
        let same =
            DgraphProjectionFrontier::delivery_operation_id("acme", "graph", "s", 3, "evt-1");
        assert_eq!(a.as_str(), same.as_str(), "a re-delivery is one operation");
        assert_ne!(
            a.as_str(),
            DgraphProjectionFrontier::delivery_operation_id("acme", "risk", "s", 3, "evt-1")
                .as_str(),
            "another consumer is another operation"
        );
        assert_ne!(
            a.as_str(),
            DgraphProjectionFrontier::delivery_operation_id("acme", "graph", "s", 4, "evt-1")
                .as_str(),
            "another position is another operation"
        );
    }

    /// The dedup key of a delta is what the replay meets: the same
    /// delta said twice is the same key; anything that differs is
    /// another record.
    #[test]
    fn the_delta_key_is_the_dedup_identity() {
        let now = Utc::now();
        let delta = RelationDelta {
            kind: hodei_evidence::domain::delta::RelationDeltaKind::Added,
            tenant: tenant(),
            relation: RelationType::Contains,
            from: hrn("Artifact", "app"),
            to: hrn("Component", "lib"),
            evidence: EvidenceId::new("evd-1"),
            cause: "evt-1".to_owned(),
            observed_at: now,
        };
        let first = DgraphProjectionFrontier::delta_key("acme", &delta);
        let same = DgraphProjectionFrontier::delta_key("acme", &delta);
        assert_eq!(first, same, "the same change said twice is one record");
        let other_cause = RelationDelta {
            cause: "evt-2".to_owned(),
            ..delta.clone()
        };
        assert_ne!(
            first,
            DgraphProjectionFrontier::delta_key("acme", &other_cause),
            "another cause is another delta"
        );
    }

    /// The receipt certifies the delta: the ledger position it came
    /// from, the next position claimed, and the deterministic
    /// fingerprint of the changeset — the same text a replay returns.
    #[test]
    fn the_receipt_carries_the_position_and_the_fingerprint() {
        let changes = changeset();
        let receipt = DgraphProjectionFrontier::receipt_text(&changes, 5);
        assert!(receipt.starts_with("pos=4;next=5;cs="), "got: {receipt}");
        let again = DgraphProjectionFrontier::receipt_text(&changes, 5);
        assert_eq!(receipt, again, "the fingerprint is deterministic");
        let other = DgraphProjectionFrontier::receipt_text(&changes, 6);
        assert_ne!(receipt, other, "another position is another receipt");
    }

    /// The parent gate names the reserved variables of the shape the
    /// attempt was built for: a fresh scope gates on the guard being
    /// absent, an existing one on the in-transaction revision.
    #[test]
    fn the_parent_gate_follows_the_guard_shape() {
        assert_eq!(
            parent_gate(0),
            "eq(len(op), 0) AND eq(len(g), 0)",
            "a fresh scope creates the guard"
        );
        assert_eq!(
            parent_gate(7),
            "eq(len(op), 0) AND eq(len(gv), 1)",
            "an existing scope revalidates its revision"
        );
    }

    /// Gating a changeset block keeps its own condition and adds the
    /// parent conjunct, so a stale or repeated write touches nothing;
    /// a block without a condition of its own still gets the parent.
    #[test]
    fn the_changeset_blocks_are_gated_without_losing_their_own() {
        let block = GraphMutationBlock::set_if(
            "eq(len(n0), 0)",
            "_:n0 <hodei.graph.node.kind> \"Artifact\" .\n".to_owned(),
        );
        let gated = gate_block(&block, parent_gate(3));
        assert_eq!(
            gated.cond.as_deref(),
            Some("eq(len(op), 0) AND eq(len(gv), 1) AND eq(len(n0), 0)"),
            "the parent conjunct rides first"
        );
        assert_eq!(gated.set_nquads, block.set_nquads, "the content is kept");
        let ungated = GraphMutationBlock {
            cond: None,
            set_nquads: "uid(q) * * .\n".to_owned(),
            delete_nquads: String::new(),
        };
        let gated = gate_block(&ungated, parent_gate(0));
        assert_eq!(
            gated.cond.as_deref(),
            Some("eq(len(op), 0) AND eq(len(g), 0)"),
            "a delete block is gated too"
        );
    }

    /// The checkpoint bindings: the create branch gates on the row
    /// being absent, the update one on the row being BEHIND the claimed
    /// position — the checkpoint never regresses, not even under a
    /// re-delivery.
    #[test]
    fn the_checkpoint_bindings_gate_create_and_behind_update() {
        let lines = checkpoint_query_lines("ck-key", 5);
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains("ck as var(func: eq(hodei.checkpoint.key, \"ck-key\"))"),
            "the unfiltered lookup: {}",
            lines[0]
        );
        assert!(
            lines[1]
                .contains("ckb as var(func: eq(hodei.checkpoint.key, \"ck-key\")) @filter(lt(hodei.checkpoint.next_position, 5))"),
            "the behind filter: {}",
            lines[1]
        );
    }

    /// The deltas of one delivery take unique, deterministic sequences:
    /// the delivery's position, plus each delta's place in the sorted
    /// dedup order — a replay recomputes the same version it first
    /// wrote, and no two deltas share a version.
    #[test]
    fn the_delta_sequences_are_deterministic_and_unique() {
        let now = Utc::now();
        let mk = |evidence: &str, relation: RelationType| RelationDelta {
            kind: hodei_evidence::domain::delta::RelationDeltaKind::Added,
            tenant: tenant(),
            relation,
            from: hrn("Artifact", "app"),
            to: hrn("Component", "lib"),
            evidence: EvidenceId::new(evidence),
            cause: "evt-1".to_owned(),
            observed_at: now,
        };
        let first = mk("evd-a", RelationType::Contains);
        let second = mk("evd-b", RelationType::Contains);
        let mut keyed: BTreeMap<String, RelationDelta> = BTreeMap::new();
        for delta in [first.clone(), second.clone(), first] {
            keyed.insert(DgraphProjectionFrontier::delta_key("acme", &delta), delta);
        }
        let planned: Vec<(u64, String)> = keyed
            .into_iter()
            .enumerate()
            .map(|(index, (key, _))| (4 + 1 + index as u64, key))
            .collect();
        assert_eq!(planned.len(), 2, "the same delta twice is planned once");
        assert_ne!(planned[0].0, planned[1].0, "no two deltas share a version");
        assert_eq!(planned[0].0, 5, "the first version is the next position");
    }

    /// The body of an upsert query is what the guarded query appends:
    /// the enclosing braces go, the bindings stay.
    #[test]
    fn the_query_body_strips_the_braces() {
        assert_eq!(
            query_body("{\n  a as var(func: uid(x))\n}\n"),
            "  a as var(func: uid(x))\n"
        );
        assert_eq!(
            query_body("{\n}\n"),
            "",
            "an empty changeset contributes nothing"
        );
    }
}
