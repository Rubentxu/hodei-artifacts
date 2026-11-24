//! The reified graph projection, over the graph authority.
//!
//! This is the Dgraph port of [`crate::surreal::graph`]: the same
//! observable semantics — whole changesets, relations reified with
//! supports, a relation whose last support was withdrawn reading as
//! UNKNOWN — mapped onto the primitives the stack already proves.
//!
//! # The mapping
//!
//! - **One node per graph node.** `HodeiGraphNode` keyed by
//!   `tenant|HRN` (exact, `@upsert`): two creators of one node conflict
//!   at the server; a replay finds its node and refreshes it.
//! - **One node per relation.** `HodeiGraphRelation` keyed by
//!   `sha256(tenant#type|from|to)`: the identity the domain gives the
//!   relation IS the identity of its node, so two evidences supporting
//!   one relation meet on ONE node, and a replay never duplicates it.
//!   The node carries `hodei.graph.rel.status`: `asserted` while it has
//!   supports, `unknown` when the last support was withdrawn.
//! - **One node per support.** `HodeiGraphSupport` keyed by
//!   `sha256(tenant#identity#evidence)`: saying the same support twice
//!   is the same support; two evidences on one relation are two
//!   supports hanging from the relation's `hodei.graph.supports` edge.
//! - **The native edge is the projection.** In the SAME transaction the
//!   relation is asserted in, the source node receives
//!   `hodei.graph.out -> relation` and the relation receives
//!   `hodei.graph.to -> target`, both reverse indexed: `relations_from`
//!   and `relations_to` are traversals, never scans. When the last
//!   support of a relation is withdrawn, the edges are DELETED, so the
//!   navigation itself stops showing an unknown relation (the status
//!   predicate keeps the state readable).
//! - **Whole or not at all.** The whole changeset is ONE upsert
//!   transaction (`commit_now`): the query block binds every entity by
//!   key and resolves, per withdrawn relation, the supports that would
//!   remain; the mutation blocks create-or-update every entity, set or
//!   retire the native edges, and decide the status under `@if`
//!   conditions. A failure aborts the transaction and nothing lands.
//! - **Chocks on the same edge.** Every apply that touches a relation
//!   writes its status predicate, so two concurrent writers of the SAME
//!   edge collide on the same data key and the server aborts the loser
//!   (`UpsertConflict`); the loser rebuilds its transaction from fresh
//!   bindings — the revalidation — and the state it then computes
//!   includes whatever the winner committed. Writers of different edges
//!   share no key and never contend.
//!
//! # Why the edges are written in four branches
//!
//! A native edge references the uids of BOTH its ends. The query block
//! binds those uids, but an entity the changeset itself creates has no
//! uid yet at query time: it is written through a blank node label, and
//! one upsert unifies its labels across mutation blocks. Whether an end
//! predates the changeset (reference it by `uid(var)`) or is co-created
//! (reference it by its blank label) is decided by the server itself,
//! with `@if(eq(len(var), 0))`: for each relation, the edge quads are
//! emitted in one branch per combination of the ends' presence. The
//! branch the server picks is always the one whose references bind.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::domain::graph::{GraphRelation, NodeKind, RelationType};
use hodei_evidence::ports::graph::{
    BoundedGraphQuery, BoundedLineage, BoundedTraversal, GraphError, GraphQuery, GraphReadLimits,
    GraphWriter, Lineage, WalkBudget,
};
use hodei_evidence::{
    EvidenceId, GraphChangeSet, GraphNode, GraphRelationWithdrawal, evidence_hrn,
};
use hodei_graph_client::{
    GraphClient, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction, dql_string,
};
use kernel::Hrn;
use sha2::{Digest, Sha256};

/// The Dgraph type of graph node records.
pub const GRAPH_NODE_TYPE: &str = "HodeiGraphNode";
/// The Dgraph type of reified relation records.
pub const GRAPH_RELATION_TYPE: &str = "HodeiGraphRelation";
/// The Dgraph type of relation support records.
pub const GRAPH_SUPPORT_TYPE: &str = "HodeiGraphSupport";

/// The relation status that means "the graph affirms this relation".
pub const STATUS_ASSERTED: &str = "asserted";
/// The relation status that means "no live support says this any more".
pub const STATUS_UNKNOWN: &str = "unknown";

/// How many times a changeset may lose the write race before giving up.
/// The bound is the Surreal adapter's [`crate::CONFLICT_RETRIES`].
const MAX_ATTEMPTS: usize = 8;

/// The name of the projection that is in use: the one the deployment
/// serves and the one the frontier emits deltas for. A rebuild builds
/// into a projection of its own name ([`DgraphGraphStore::named`]); the
/// name rides on every record (`hodei.graph.projection`) and, for the
/// named ones, on the front of every identity key, so the two
/// projections never meet — neither by key nor by scan — and the graph
/// in use keeps answering while the rebuild runs.
pub const ACTIVE_PROJECTION: &str = "graph";

/// The graph of evidence, over the graph authority.
#[derive(Debug)]
pub struct DgraphGraphStore {
    client: Arc<GraphClient>,
    projection: String,
}

impl DgraphGraphStore {
    /// The store over a connected, schema-validated client. The schema
    /// boot is the caller's readiness gate, like every adapter of this
    /// crate; the store itself carries no state.
    pub fn new(client: Arc<GraphClient>) -> Self {
        Self::named(client, ACTIVE_PROJECTION)
    }

    /// A projection with a name of its own: the rebuild builds into one,
    /// and the deployment promotes it by serving it, the same decision
    /// the Surreal adapter leaves to whoever orchestrates the rebuild.
    pub fn named(client: Arc<GraphClient>, projection: impl Into<String>) -> Self {
        Self {
            client,
            projection: projection.into(),
        }
    }

    /// The name of the projection this store serves.
    pub fn projection(&self) -> &str {
        &self.projection
    }

    /// The prefix the identity keys of THIS projection carry. The active
    /// projection keeps the bare keys the projection has always had; a
    /// named one namespaces its own so a lookup can never meet the
    /// active record of the same identity.
    fn key_prefix(&self) -> String {
        if self.projection == ACTIVE_PROJECTION {
            String::new()
        } else {
            format!("{}|", self.projection)
        }
    }

    /// The identity key of a graph node of this projection.
    fn node_key_scoped(&self, tenant_hrn: &str, node_hrn: &str) -> String {
        format!(
            "{}{}",
            self.key_prefix(),
            Self::node_key(tenant_hrn, node_hrn)
        )
    }

    /// The identity key of a graph node: the account and the HRN, the
    /// same composite the identity records use. HRNs never carry the
    /// separator, so the key has one parsing.
    pub fn node_key(tenant_hrn: &str, node_hrn: &str) -> String {
        format!("{tenant_hrn}|{node_hrn}")
    }

    /// The identity key of a reified relation: a digest, because the
    /// relation identity (`type|from|to`) itself carries the composite
    /// separator.
    pub fn relation_key(tenant_hrn: &str, identity: &str) -> String {
        sha256_key(&format!("{tenant_hrn}#{identity}"))
    }

    /// The identity key of one support: the relation AND the evidence,
    /// so the same support said twice is the same record and two
    /// evidences on one relation are two records.
    pub fn support_key(tenant_hrn: &str, identity: &str, evidence: &str) -> String {
        sha256_key(&format!("{tenant_hrn}#{identity}#{evidence}"))
    }

    /// The fingerprint of a change set: deterministic, so a replay of
    /// the same changeset carries the same fingerprint.
    pub fn changeset_fingerprint(changes: &GraphChangeSet) -> String {
        let mut canonical = String::new();
        for node in &changes.nodes {
            canonical.push_str(&format!(
                "N|{}|{}|{}\n",
                node.id,
                node.kind,
                node.observed_at.to_rfc3339()
            ));
        }
        for relation in &changes.relations {
            canonical.push_str(&format!(
                "R|{}|{}|{}|{}\n",
                relation.identity(),
                relation.evidence_id.as_str(),
                relation.tenant_hrn,
                relation.observed_at.to_rfc3339()
            ));
        }
        for withdrawal in &changes.withdrawals {
            canonical.push_str(&format!(
                "W|{}|{}|{}\n",
                withdrawal.identity(),
                withdrawal.evidence_id.as_str(),
                withdrawal.observed_at.to_rfc3339()
            ));
        }
        if let Some(cause) = &changes.cause {
            canonical.push_str(&format!("C|{cause}\n"));
        }
        sha256_key(&canonical)
    }

    /// The state fingerprint of the graph of one account: the canonical
    /// digest of every node, every ASSERTED relation and the live
    /// supports. Two graphs that answer questions the same way carry
    /// the same fingerprint, and a replay of a changeset must not move
    /// it.
    pub async fn state_fingerprint(&self, tenant: &Hrn) -> Result<String, GraphError> {
        let mut all_nodes = Vec::new();
        for kind in [
            NodeKind::Repository,
            NodeKind::SourceRevision,
            NodeKind::Build,
            NodeKind::Artifact,
            NodeKind::Deployment,
            NodeKind::Evidence,
            NodeKind::Component,
            NodeKind::Vulnerability,
        ] {
            all_nodes.extend(self.nodes(tenant, kind).await?);
        }
        all_nodes.sort_by_key(|node| node.id.to_string());

        // The asserted relations of the account, walked natively from
        // every node, deduplicated by identity.
        let mut relations: BTreeMap<String, String> = BTreeMap::new();
        for node in &all_nodes {
            for relation in self.relations_from(tenant, &node.id).await? {
                relations.insert(
                    relation.identity(),
                    format!(
                        "{}|{}|{}",
                        relation.identity(),
                        relation.evidence_id.as_str(),
                        relation.observed_at.to_rfc3339()
                    ),
                );
            }
        }

        // The live supports, read from the asserted relations. The scan
        // is the projection's own: another projection building apart
        // holds records the fingerprint must never see.
        let tenant_literal = dql_string(&tenant.to_string());
        let query = format!(
            "{{ q(func: eq(hodei.graph.rel.tenant, {tenant_literal})) \
             @filter(eq(hodei.graph.rel.status, {status}) AND eq(hodei.graph.projection, {projection})) {{ \
             hodei.graph.rel.key hodei.graph.supports {{ hodei.graph.support.evidence }} }} }}",
            status = dql_string(STATUS_ASSERTED),
            projection = dql_string(&self.projection),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let mut supports: Vec<String> = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .flat_map(|relation| {
                let rel_key = relation
                    .get("hodei.graph.rel.key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                relation
                    .get("hodei.graph.supports")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |support| {
                        format!(
                            "{rel_key}|{}",
                            support
                                .get("hodei.graph.support.evidence")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                        )
                    })
            })
            .collect();
        supports.sort();

        let mut canonical = String::new();
        for node in &all_nodes {
            canonical.push_str(&format!("N|{}|{}\n", node.id, node.kind));
        }
        for line in relations.values() {
            canonical.push_str(&format!("R|{line}\n"));
        }
        for support in supports {
            canonical.push_str(&format!("S|{support}\n"));
        }
        Ok(sha256_key(&canonical))
    }

    /// Whether the graph holds the node with this identity, under this
    /// account.
    async fn node_exists(&self, tenant: &Hrn, id: &Hrn) -> Result<bool, GraphError> {
        let key = self.node_key_scoped(&tenant.to_string(), &id.to_string());
        let query = format!(
            "{{ q(func: eq(hodei.graph.node.key, {})) {{ uid }} }}",
            dql_string(&key)
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let count = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .map(|nodes| nodes.len())
            .unwrap_or(0);
        Ok(count > 0)
    }

    /// The relations reached through one navigable edge, of an account.
    ///
    /// The traversal is native: the relations hang from the node by
    /// `hodei.graph.out` (forward) or `~hodei.graph.to` (backward), and
    /// the account and the status are filters on the expansion. An
    /// unknown relation hangs no edge, so it is never walked — the same
    /// answer the Surreal adapter gives by its status filter.
    async fn relations_via(
        &self,
        tenant: &Hrn,
        node: &Hrn,
        forward: bool,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        let tenant_literal = dql_string(&tenant.to_string());
        let node_key = self.node_key_scoped(&tenant.to_string(), &node.to_string());
        let edge = if forward {
            "hodei.graph.out"
        } else {
            "~hodei.graph.to"
        };
        let query = format!(
            "{{ q(func: eq(hodei.graph.node.key, {node_key_literal})) {{\n\
               {edge} @filter(eq(hodei.graph.rel.tenant, {tenant_literal}) AND \
             eq(hodei.graph.rel.status, {status})) {{\n\
               {RELATION_FIELDS}\n\
               }}\n\
             }} }}",
            node_key_literal = dql_string(&node_key),
            status = dql_string(STATUS_ASSERTED),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows = response
            .json
            .get("q")
            .and_then(|q| q.get(0))
            .and_then(|node| node.get(edge))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(rows.iter().filter_map(relation_of).collect())
    }

    /// A node of a kind, by its identity, of an account.
    ///
    /// The account and the kind are checked on the row that comes back,
    /// the same way the Surreal adapter checks them.
    async fn node_of_kind(
        &self,
        tenant: &Hrn,
        id: &Hrn,
        kind: NodeKind,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let node_key = self.node_key_scoped(&tenant.to_string(), &id.to_string());
        let query = format!(
            "{{ q(func: eq(hodei.graph.node.key, {})) {{ {NODE_FIELDS} }} }}",
            dql_string(&node_key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(rows
            .iter()
            .filter(|row| {
                row.get("hodei.graph.node.tenant")
                    .and_then(serde_json::Value::as_str)
                    == Some(tenant.to_string().as_str())
            })
            .filter(|row| {
                row.get("hodei.graph.node.kind")
                    .and_then(serde_json::Value::as_str)
                    == Some(kind.as_str())
            })
            .filter_map(node_of)
            .collect())
    }
}

/// The reference of one end of an edge, under the presence branch the
/// server picks for it. The reference is `uid(var)` when the end
/// predates the changeset and the blank label when the changeset
/// co-creates it.
#[derive(Debug, Clone)]
struct EndpointRef {
    /// The query variable of the endpoint node (`n3`).
    var: String,
    /// The identity key the variable binds on.
    key: String,
    /// Whether the endpoint's content is written by this changeset
    /// (`true`). Endpoints that are only bound for their uid get a
    /// minimal record (key and account) when they turn out to be new,
    /// so the edge can be walked and later changesets can bind them.
    in_changeset: bool,
}

impl EndpointRef {
    /// The reference under the branch where the endpoint does not exist
    /// before this transaction.
    fn as_blank(&self) -> String {
        format!("_:{}", self.var)
    }

    /// The reference under the branch where the endpoint exists.
    fn as_uid(&self) -> String {
        format!("uid({})", self.var)
    }

    /// The `@if` conjunct that says whether the endpoint exists.
    fn exists_cond(&self) -> String {
        format!("eq(len({}), 1)", self.var)
    }

    /// The `@if` conjunct that says whether the endpoint is new.
    fn absent_cond(&self) -> String {
        format!("eq(len({}), 0)", self.var)
    }
}

/// The upsert plan of one changeset: every entity resolved to its key,
/// its bindings and its branches, so the transaction builder stays
/// declarative.
struct ChangePlan<'a> {
    /// The nodes the changeset UPSERTS, with their key and index.
    nodes: Vec<(usize, String, &'a GraphNode)>,
    /// Every endpoint any relation or withdrawal names, by HRN.
    endpoints: BTreeMap<String, EndpointRef>,
    /// The reified relations, one per identity, with their supports.
    relations: Vec<(usize, String, &'a GraphRelation, Vec<&'a GraphRelation>)>,
    /// The withdrawal decisions, one per withdrawn identity.
    withdrawals: Vec<(usize, String, &'a GraphRelationWithdrawal, Vec<String>)>,
}

/// Builds the plan of a changeset. Identities are deduplicated: two
/// evidences on one relation are ONE relation with TWO supports; the
/// same support withdrawn by two entries is ONE deletion. The `prefix`
/// namespaces every identity key, so a named projection plans against
/// its own records.
fn plan_changeset<'a>(changes: &'a GraphChangeSet, prefix: &'a str) -> ChangePlan<'a> {
    let mut node_count = 0usize;
    let mut node_index_of_key: BTreeMap<String, usize> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut endpoints: BTreeMap<String, EndpointRef> = BTreeMap::new();

    // The nodes the changeset carries: upserted and bound.
    for node in &changes.nodes {
        let key = format!(
            "{prefix}{}",
            DgraphGraphStore::node_key(&node.tenant_hrn.to_string(), &node.id.to_string())
        );
        let index = *node_index_of_key.entry(key.clone()).or_insert(node_count);
        if index == node_count {
            node_count += 1;
            nodes.push((index, key.clone(), node));
        }
        endpoints.insert(
            node.id.to_string(),
            EndpointRef {
                var: format!("n{index}"),
                key,
                in_changeset: true,
            },
        );
    }
    // The endpoints named by relations and withdrawals that the
    // changeset does not carry: bound for their uid, never upserted.
    for relation in &changes.relations {
        for endpoint in [&relation.from, &relation.to] {
            endpoints.entry(endpoint.to_string()).or_insert_with(|| {
                let key = format!(
                    "{prefix}{}",
                    DgraphGraphStore::node_key(
                        &relation.tenant_hrn.to_string(),
                        &endpoint.to_string(),
                    )
                );
                let index = *node_index_of_key.entry(key.clone()).or_insert(node_count);
                if index == node_count {
                    node_count += 1;
                }
                EndpointRef {
                    var: format!("n{index}"),
                    key,
                    in_changeset: false,
                }
            });
        }
    }
    for withdrawal in &changes.withdrawals {
        for endpoint in [&withdrawal.from, &withdrawal.to] {
            endpoints.entry(endpoint.to_string()).or_insert_with(|| {
                let key = format!(
                    "{prefix}{}",
                    DgraphGraphStore::node_key(
                        &withdrawal.tenant_hrn.to_string(),
                        &endpoint.to_string(),
                    )
                );
                let index = *node_index_of_key.entry(key.clone()).or_insert(node_count);
                if index == node_count {
                    node_count += 1;
                }
                EndpointRef {
                    var: format!("n{index}"),
                    key,
                    in_changeset: false,
                }
            });
        }
    }

    // The relations: one plan entry per identity, with every support
    // the changeset asserts on it.
    let mut relations = Vec::new();
    let mut relation_index_of: BTreeMap<String, usize> = BTreeMap::new();
    for relation in &changes.relations {
        let identity = relation.identity();
        let index = *relation_index_of
            .entry(identity.clone())
            .or_insert(relations.len());
        if index == relations.len() {
            relations.push((index, identity.clone(), relation, Vec::new()));
        }
        relations[index].3.push(relation);
    }

    // The withdrawals: one decision per identity, with every evidence
    // withdrawn from it, and the supports the changeset adds to the
    // SAME identity (they are written by this same transaction and
    // cannot be counted by its query).
    let mut withdrawals = Vec::new();
    let mut withdrawal_index_of: BTreeMap<String, usize> = BTreeMap::new();
    for withdrawal in &changes.withdrawals {
        let identity = withdrawal.identity();
        let index = *withdrawal_index_of
            .entry(identity.clone())
            .or_insert(withdrawals.len());
        if index == withdrawals.len() {
            withdrawals.push((index, identity.clone(), withdrawal, Vec::new()));
        }
        let evidence = withdrawal.evidence_id.as_str().to_owned();
        if !withdrawals[index].3.contains(&evidence) {
            withdrawals[index].3.push(evidence);
        }
    }

    ChangePlan {
        nodes,
        endpoints,
        relations,
        withdrawals,
    }
}

impl DgraphGraphStore {
    /// The upsert transaction of one changeset.
    ///
    /// The remaining-support decision of a withdrawal is `remaining > 0`
    /// where `remaining = |pre - withdrawn| + |added - withdrawn|` and
    /// `added` are the supports this same transaction writes for the
    /// relation (the query cannot see them, so the client folds their
    /// number into the decision). The sum never flips the decision: a
    /// support that is both pre-existing and re-added and not withdrawn
    /// IS a live one, so the only possible overcount still answers
    /// `asserted`, and every live support is counted by at least one of
    /// the two terms.
    /// The upsert transaction of one changeset, for the projection the
    /// caller names. The frontier composes it into the guarded
    /// transaction that also carries the deltas and the checkpoint, so
    /// it is shared with [`crate::dgraph::frontier`].
    pub(crate) fn changeset_upsert_scoped(
        changes: &GraphChangeSet,
        prefix: &str,
        projection: &str,
    ) -> GraphUpsertTransaction {
        let plan = plan_changeset(changes, prefix);
        let mut query = String::from("{\n");
        let mut blocks: Vec<GraphMutationBlock> = Vec::new();

        // -- Query block ----------------------------------------------------
        // Every bound endpoint gets its binding, whether its content is
        // upserted here or it is bound for its uid alone.
        for (index, key, _node) in &plan.nodes {
            query.push_str(&format!(
                "  n{index} as var(func: eq(hodei.graph.node.key, {}))\n",
                dql_string(key)
            ));
        }
        for endpoint in plan.endpoints.values() {
            if !endpoint.in_changeset {
                query.push_str(&format!(
                    "  {} as var(func: eq(hodei.graph.node.key, {}))\n",
                    endpoint.var,
                    dql_string(&endpoint.key)
                ));
            }
        }
        for (index, identity, relation, supports) in &plan.relations {
            query.push_str(&format!(
                "  r{index} as var(func: eq(hodei.graph.rel.key, {}))\n",
                dql_string(&format!(
                    "{prefix}{}",
                    Self::relation_key(&relation.tenant_hrn.to_string(), identity)
                ))
            ));
            for (position, support) in supports.iter().enumerate() {
                let key = format!(
                    "{prefix}{}",
                    Self::support_key(
                        &support.tenant_hrn.to_string(),
                        identity,
                        support.evidence_id.as_str(),
                    )
                );
                query.push_str(&format!(
                    "  s{index}_{position} as var(func: eq(hodei.graph.support.key, {}))\n",
                    dql_string(&key)
                ));
            }
        }
        for (index, identity, withdrawal, evidences) in &plan.withdrawals {
            for (position, evidence) in evidences.iter().enumerate() {
                let key = format!(
                    "{prefix}{}",
                    Self::support_key(&withdrawal.tenant_hrn.to_string(), identity, evidence)
                );
                query.push_str(&format!(
                    "  w{index}_{position} as var(func: eq(hodei.graph.support.key, {}))\n",
                    dql_string(&key)
                ));
            }
            // The relation node whose status is recomputed: bound here
            // when the changeset does not assert it.
            let rel_var = match plan
                .relations
                .iter()
                .find(|(_, plan_identity, _, _)| plan_identity == identity)
            {
                Some((relation_index, _, _, _)) => format!("r{relation_index}"),
                None => format!("rw{index}"),
            };
            if rel_var.starts_with("rw") {
                query.push_str(&format!(
                    "  {rel_var} as var(func: eq(hodei.graph.rel.key, {}))\n",
                    dql_string(&format!(
                        "{prefix}{}",
                        Self::relation_key(&withdrawal.tenant_hrn.to_string(), identity)
                    ))
                ));
            }
            query.push_str(&format!(
                "  var(func: uid({rel_var})) {{ wsup{index} as hodei.graph.supports }}\n"
            ));
            let mut filter = String::new();
            for (position, evidence) in evidences.iter().enumerate() {
                if position > 0 {
                    filter.push_str(" AND ");
                }
                filter.push_str(&format!(
                    "NOT eq(hodei.graph.support.evidence, {})",
                    dql_string(evidence)
                ));
            }
            query.push_str(&format!(
                "  wleft{index} as var(func: uid(wsup{index})) @filter({filter})\n"
            ));
        }
        query.push_str("}\n");

        // -- Node mutations --------------------------------------------------
        // Create when absent, refresh when present. Nodes that are only
        // bound for their uid get no content writes.
        for (index, _key, node) in &plan.nodes {
            blocks.push(GraphMutationBlock::set_if(
                format!("eq(len(n{index}), 0)"),
                node_nquads(&format!("n{index}"), node, prefix, projection),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("eq(len(n{index}), 1)"),
                node_update_nquads(&format!("n{index}"), node, projection),
            ));
        }

        // -- Relation mutations ----------------------------------------------
        for (index, identity, relation, supports) in &plan.relations {
            let from = plan
                .endpoints
                .get(&relation.from.to_string())
                .expect("the endpoints of every relation are bound by the plan");
            let to = plan
                .endpoints
                .get(&relation.to.to_string())
                .expect("the endpoints of every relation are bound by the plan");

            // The relation record, created when absent and refreshed
            // when present. The native edges live in their own presence
            // branches below, because each end may predate the
            // changeset or be co-created by it.
            blocks.push(GraphMutationBlock::set_if(
                format!("eq(len(r{index}), 0)"),
                relation_nquads(&format!("r{index}"), relation, identity, prefix, projection),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("eq(len(r{index}), 1)"),
                relation_update_nquads(&format!("r{index}"), relation, projection),
            ));

            // The native edges, one branch per presence combination of
            // the two ends (and the relation itself). The branch the
            // server picks is always the one whose references bind: an
            // end that predates the changeset is referenced by uid, an
            // end the changeset co-creates by its blank label, which
            // one upsert unifies across its blocks. An end that is only
            // bound for its uid gets a minimal record (key and account)
            // in the branch where it turns out to be new, so the edge
            // is walkable and later changesets can bind it.
            let husk = |endpoint: &EndpointRef| {
                if endpoint.in_changeset {
                    String::new()
                } else {
                    format!(
                        "_:{} <hodei.graph.node.key> {} .\n\
                         _:{} <hodei.graph.node.tenant> {} .\n\
                         _:{} <hodei.graph.projection> {} .\n",
                        endpoint.var,
                        dql_string(&endpoint.key),
                        endpoint.var,
                        dql_string(&relation.tenant_hrn.to_string()),
                        endpoint.var,
                        dql_string(projection),
                    )
                }
            };
            for (from_ref, from_cond) in [
                (from.as_uid(), from.exists_cond()),
                (from.as_blank(), from.absent_cond()),
            ] {
                for (to_ref, to_cond) in [
                    (to.as_uid(), to.exists_cond()),
                    (to.as_blank(), to.absent_cond()),
                ] {
                    let mut create_edges = husk(from);
                    create_edges.push_str(&husk(to));
                    create_edges.push_str(&format!(
                        "_:r{index} <hodei.graph.to> {to_ref} .\n\
                         {from_ref} <hodei.graph.out> _:r{index} .\n"
                    ));
                    blocks.push(GraphMutationBlock::set_if(
                        format!("eq(len(r{index}), 0) AND {from_cond} AND {to_cond}"),
                        create_edges,
                    ));
                    blocks.push(GraphMutationBlock::set_if(
                        format!("eq(len(r{index}), 1) AND {from_cond} AND {to_cond}"),
                        format!(
                            "uid(r{index}) <hodei.graph.to> {to_ref} .\n\
                             {from_ref} <hodei.graph.out> uid(r{index}) .\n"
                        ),
                    ));
                }
            }

            // The supports. The relation that is co-created links them
            // with its own blank label (the labels unify); the relation
            // that predates the changeset links the new ones by its
            // uid, and relinks the existing ones.
            for (position, support) in supports.iter().enumerate() {
                let support_var = format!("s{index}_{position}");
                blocks.push(GraphMutationBlock::set_if(
                    format!("eq(len({support_var}), 0)"),
                    support_create_nquads(
                        &support_var,
                        support,
                        identity,
                        Some(format!("uid(r{index})")),
                        prefix,
                        projection,
                    ),
                ));
                blocks.push(GraphMutationBlock::set_if(
                    format!("eq(len({support_var}), 1)"),
                    format!(
                        "uid(r{index}) <hodei.graph.supports> uid({support_var}) .\n\
                         uid({support_var}) <hodei.graph.support.evidence> {} .\n\
                         uid({support_var}) <hodei.graph.support.observed_at> {} .\n\
                         uid({support_var}) <hodei.graph.projection> {} .\n",
                        dql_string(support.evidence_id.as_str()),
                        dql_string(&support.observed_at.to_rfc3339()),
                        dql_string(projection),
                    ),
                ));
                // The link from a co-created relation: written in its
                // create branch, where the blank labels unify with the
                // support's own create block.
                blocks.push(GraphMutationBlock::set_if(
                    format!("eq(len(r{index}), 0) AND eq(len({support_var}), 0)"),
                    format!("_:r{index} <hodei.graph.supports> _:{support_var} .\n"),
                ));
            }
        }

        // -- Withdrawal mutations --------------------------------------------
        for (index, identity, withdrawal, evidences) in &plan.withdrawals {
            let rel_var = match plan
                .relations
                .iter()
                .find(|(_, plan_identity, _, _)| plan_identity == identity)
            {
                Some((relation_index, _, _, _)) => format!("r{relation_index}"),
                None => format!("rw{index}"),
            };

            // The support nodes the withdrawal names go away, whatever
            // the remaining count says: deleting an absent support is a
            // no-op, which is what makes a replay of a withdrawal
            // idempotent.
            let mut deletes = String::new();
            for position in 0..evidences.len() {
                deletes.push_str(&format!("uid(w{index}_{position}) * * .\n"));
                deletes.push_str(&format!(
                    "uid({rel_var}) <hodei.graph.supports> uid(w{index}_{position}) .\n"
                ));
            }
            blocks.push(GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: deletes,
            });

            // The relation gains supports in this same transaction: its
            // add blocks already wrote `asserted` and the edges, and
            // the client-folded count is what keeps the answer right —
            // no recomputation here.
            let added_here = changes
                .relations
                .iter()
                .filter(|added| added.identity() == *identity)
                .filter(|added| !evidences.contains(&added.evidence_id.as_str().to_owned()))
                .count();
            if added_here > 0 {
                continue;
            }

            let from = plan.endpoints.get(&withdrawal.from.to_string());
            let to = plan.endpoints.get(&withdrawal.to.to_string());

            // Supports remain: the relation is asserted, the edges hold.
            // The ends carried the edges being kept, so they existed
            // before this transaction and the uid references bind.
            let mut keep = format!(
                "uid({rel_var}) <hodei.graph.rel.status> {} .\n",
                dql_string(STATUS_ASSERTED),
            );
            if let (Some(from), Some(to)) = (from, to) {
                keep.push_str(&format!(
                    "uid({rel_var}) <hodei.graph.to> {} .\n\
                     {} <hodei.graph.out> uid({rel_var}) .\n",
                    to.as_uid(),
                    from.as_uid(),
                ));
            }
            blocks.push(GraphMutationBlock::set_if(
                format!("gt(len(wleft{index}), 0)"),
                keep,
            ));

            // Nothing remains: UNKNOWN, and the native edges are
            // retired, so the navigation stops showing the relation.
            // The edge retirement is a DELETE (the star form), the
            // status is a set: one block, both halves.
            let retired_status = format!(
                "uid({rel_var}) <hodei.graph.rel.status> {} .\n",
                dql_string(STATUS_UNKNOWN),
            );
            let mut retired_edges = format!("uid({rel_var}) <hodei.graph.to> * .\n");
            if let Some(from) = from {
                retired_edges.push_str(&format!(
                    "{} <hodei.graph.out> uid({rel_var}) .\n",
                    from.as_uid()
                ));
            }
            blocks.push(GraphMutationBlock {
                cond: Some(format!("eq(len(wleft{index}), 0)")),
                set_nquads: retired_status,
                delete_nquads: retired_edges,
            });
        }

        GraphUpsertTransaction {
            query,
            blocks,
            commit_now: true,
        }
    }
}

/// The fields every graph node record is read with.
const NODE_FIELDS: &str = "hodei.graph.node.key hodei.graph.node.tenant hodei.graph.node.kind \
     hodei.graph.node.attributes hodei.graph.node.observed_at";

/// The fields every relation record is read with.
const RELATION_FIELDS: &str = "hodei.graph.rel.tenant hodei.graph.rel.type \
     hodei.graph.rel.from_hrn hodei.graph.rel.to_hrn hodei.graph.rel.evidence \
     hodei.graph.rel.observed_at";

fn sha256_key(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// The node of a stored record. The key carries the account and the
/// HRN; a row whose key does not parse is corruption, filtered out.
fn node_of(row: &serde_json::Value) -> Option<GraphNode> {
    let key = row
        .get("hodei.graph.node.key")
        .and_then(serde_json::Value::as_str)?;
    let hrn = key.rsplit_once('|')?.1;
    let id = Hrn::from_string(hrn)?;
    let tenant = Hrn::from_string(
        row.get("hodei.graph.node.tenant")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let kind = NodeKind::parse(
        row.get("hodei.graph.node.kind")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let attributes: serde_json::Map<String, serde_json::Value> = row
        .get("hodei.graph.node.attributes")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default();
    let observed_at = observed_at_of(row, "hodei.graph.node.observed_at");
    Some(GraphNode {
        id,
        tenant_hrn: tenant,
        kind,
        attributes,
        observed_at,
    })
}

/// The relation of a stored record.
fn relation_of(row: &serde_json::Value) -> Option<GraphRelation> {
    let relation = RelationType::parse(
        row.get("hodei.graph.rel.type")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let from = Hrn::from_string(
        row.get("hodei.graph.rel.from_hrn")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let to = Hrn::from_string(
        row.get("hodei.graph.rel.to_hrn")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let tenant = Hrn::from_string(
        row.get("hodei.graph.rel.tenant")
            .and_then(serde_json::Value::as_str)?,
    )?;
    let evidence = row
        .get("hodei.graph.rel.evidence")
        .and_then(serde_json::Value::as_str)?
        .to_owned();
    let observed_at = observed_at_of(row, "hodei.graph.rel.observed_at");
    Some(GraphRelation {
        relation,
        from,
        to,
        tenant_hrn: tenant,
        evidence_id: EvidenceId::new(evidence),
        observed_at,
    })
}

/// The observed instant of a row, read back from its RFC 3339 text.
fn observed_at_of(row: &serde_json::Value, field: &str) -> chrono::DateTime<chrono::Utc> {
    row.get(field)
        .and_then(serde_json::Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|instant| instant.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now)
}

/// The N-Quads that create one graph node, by its blank label.
fn node_nquads(blank: &str, node: &GraphNode, prefix: &str, projection: &str) -> String {
    let attributes = serde_json::to_string(&node.attributes).unwrap_or_default();
    format!(
        "_:{blank} <dgraph.type> \"{GRAPH_NODE_TYPE}\" .\n\
         _:{blank} <hodei.graph.node.key> {} .\n\
         _:{blank} <hodei.graph.node.tenant> {} .\n\
         _:{blank} <hodei.graph.node.kind> {} .\n\
         _:{blank} <hodei.graph.node.attributes> {} .\n\
         _:{blank} <hodei.graph.node.observed_at> {} .\n\
         _:{blank} <hodei.graph.projection> {} .\n",
        dql_string(&format!(
            "{prefix}{}",
            DgraphGraphStore::node_key(&node.tenant_hrn.to_string(), &node.id.to_string())
        )),
        dql_string(&node.tenant_hrn.to_string()),
        dql_string(node.kind.as_str()),
        dql_string(&attributes),
        dql_string(&node.observed_at.to_rfc3339()),
        dql_string(projection),
    )
}

/// The N-Quads that refresh the fields of one graph node, by its uid.
fn node_update_nquads(var: &str, node: &GraphNode, projection: &str) -> String {
    let attributes = serde_json::to_string(&node.attributes).unwrap_or_default();
    format!(
        "uid({var}) <hodei.graph.node.tenant> {} .\n\
         uid({var}) <hodei.graph.node.kind> {} .\n\
         uid({var}) <hodei.graph.node.attributes> {} .\n\
         uid({var}) <hodei.graph.node.observed_at> {} .\n\
         uid({var}) <hodei.graph.projection> {} .\n",
        dql_string(&node.tenant_hrn.to_string()),
        dql_string(node.kind.as_str()),
        dql_string(&attributes),
        dql_string(&node.observed_at.to_rfc3339()),
        dql_string(projection),
    )
}

/// The N-Quads that CREATE one relation record: the identity key and
/// the asserted status, with both ends recorded as text.
fn relation_nquads(
    blank: &str,
    relation: &GraphRelation,
    identity: &str,
    prefix: &str,
    projection: &str,
) -> String {
    format!(
        "_:{blank} <dgraph.type> \"{GRAPH_RELATION_TYPE}\" .\n\
         _:{blank} <hodei.graph.rel.key> {} .\n\
         _:{blank} <hodei.graph.rel.tenant> {} .\n\
         _:{blank} <hodei.graph.rel.type> {} .\n\
         _:{blank} <hodei.graph.rel.status> {} .\n\
         _:{blank} <hodei.graph.rel.from_hrn> {} .\n\
         _:{blank} <hodei.graph.rel.to_hrn> {} .\n\
         _:{blank} <hodei.graph.rel.evidence> {} .\n\
         _:{blank} <hodei.graph.rel.observed_at> {} .\n\
         _:{blank} <hodei.graph.projection> {} .\n",
        dql_string(&format!(
            "{prefix}{}",
            DgraphGraphStore::relation_key(&relation.tenant_hrn.to_string(), identity)
        )),
        dql_string(&relation.tenant_hrn.to_string()),
        dql_string(relation.relation.as_str()),
        dql_string(STATUS_ASSERTED),
        dql_string(&relation.from.to_string()),
        dql_string(&relation.to.to_string()),
        dql_string(relation.evidence_id.as_str()),
        dql_string(&relation.observed_at.to_rfc3339()),
        dql_string(projection),
    )
}

/// The N-Quads that refresh one relation record: the fields, and the
/// status re-affirmed (the writers of one relation never disagree about
/// `asserted` here; the withdrawal branches write `unknown` when and
/// only when nothing remains).
fn relation_update_nquads(var: &str, relation: &GraphRelation, projection: &str) -> String {
    format!(
        "uid({var}) <hodei.graph.rel.tenant> {} .\n\
         uid({var}) <hodei.graph.rel.type> {} .\n\
         uid({var}) <hodei.graph.rel.status> {} .\n\
         uid({var}) <hodei.graph.rel.from_hrn> {} .\n\
         uid({var}) <hodei.graph.rel.to_hrn> {} .\n\
         uid({var}) <hodei.graph.rel.evidence> {} .\n\
         uid({var}) <hodei.graph.rel.observed_at> {} .\n\
         uid({var}) <hodei.graph.projection> {} .\n",
        dql_string(&relation.tenant_hrn.to_string()),
        dql_string(relation.relation.as_str()),
        dql_string(STATUS_ASSERTED),
        dql_string(&relation.from.to_string()),
        dql_string(&relation.to.to_string()),
        dql_string(relation.evidence_id.as_str()),
        dql_string(&relation.observed_at.to_rfc3339()),
        dql_string(projection),
    )
}

/// The N-Quads that create one support record, linked from its relation
/// by uid (the relation predates this transaction; when it is
/// co-created, its own create branch writes the link with blank labels
/// that unify with these).
fn support_create_nquads(
    blank: &str,
    support: &GraphRelation,
    identity: &str,
    from_relation: Option<String>,
    prefix: &str,
    projection: &str,
) -> String {
    let mut nquads = String::new();
    if let Some(subject) = from_relation {
        nquads.push_str(&format!("{subject} <hodei.graph.supports> _:{blank} .\n"));
    }
    nquads.push_str(&format!(
        "_:{blank} <dgraph.type> \"{GRAPH_SUPPORT_TYPE}\" .\n\
         _:{blank} <hodei.graph.support.key> {} .\n\
         _:{blank} <hodei.graph.support.evidence> {} .\n\
         _:{blank} <hodei.graph.support.observed_at> {} .\n\
         _:{blank} <hodei.graph.projection> {} .\n",
        dql_string(&format!(
            "{prefix}{}",
            DgraphGraphStore::support_key(
                &support.tenant_hrn.to_string(),
                identity,
                support.evidence_id.as_str()
            )
        )),
        dql_string(support.evidence_id.as_str()),
        dql_string(&support.observed_at.to_rfc3339()),
        dql_string(projection),
    ));
    nquads
}

#[async_trait]
impl GraphWriter for DgraphGraphStore {
    /// Applies a change set: whole or not at all.
    ///
    /// The validation runs before anything is written — a relation that
    /// claims evidence the graph does not hold is a claim, and a claim
    /// is not part of a graph that answers questions about trust. Then
    /// ONE upsert transaction writes every node, every reified relation
    /// with its supports and its native edges, and recomputes the status
    /// of every withdrawn relation from the supports that are LEFT: with
    /// supports it stays an assertion; without them what remains is
    /// UNKNOWN and the edge is retired.
    async fn apply(&self, changes: GraphChangeSet) -> Result<(), GraphError> {
        for relation in &changes.relations {
            let evidence = evidence_hrn(&relation.tenant_hrn, &relation.evidence_id);
            let known = changes.nodes.iter().any(|node| node.id == evidence)
                || self.node_exists(&relation.tenant_hrn, &evidence).await?;
            if !known {
                return Err(GraphError::Rejected(format!(
                    "the relation claims the evidence {} and the graph does not have it",
                    relation.evidence_id.as_str()
                )));
            }
        }
        if changes.is_empty() {
            return Ok(());
        }

        // The transaction is rebuilt per attempt: a conflict means
        // another writer of one of the same keys committed first, and
        // the fresh bindings are the revalidation — the same loop the
        // Surreal adapter runs on its SQL conflicts.
        for attempt in 0..MAX_ATTEMPTS {
            match self
                .client
                .upsert(Self::changeset_upsert_scoped(
                    &changes,
                    &self.key_prefix(),
                    &self.projection,
                ))
                .await
            {
                Ok(_) => return Ok(()),
                Err(hodei_graph_client::GraphClientError::UpsertConflict { detail })
                    if attempt + 1 < MAX_ATTEMPTS =>
                {
                    tracing::debug!(attempt, %detail, "the changeset write conflicted; revalidating");
                    tokio::time::sleep(std::time::Duration::from_millis(
                        10u64 << attempt.min(6) as u32,
                    ))
                    .await;
                }
                Err(error) => return Err(GraphError::Unavailable(error.to_string())),
            }
        }
        Err(GraphError::Rejected(format!(
            "the change set did not commit after {MAX_ATTEMPTS} attempts"
        )))
    }

    /// Empties the projection: every node, every relation, every
    /// support, in one transaction. The schema is untouched, so the
    /// graph can be rebuilt from the ledger right after.
    async fn clear(&self) -> Result<(), GraphError> {
        let transaction = GraphUpsertTransaction {
            query: "{\n  q as var(func: type(HodeiGraphNode))\n  \
                    r as var(func: type(HodeiGraphRelation))\n  \
                    s as var(func: type(HodeiGraphSupport))\n}"
                .to_owned(),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: "uid(q) * * .\nuid(r) * * .\nuid(s) * * .\n".to_owned(),
            }],
            commit_now: true,
        };
        self.client
            .upsert(transaction)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl GraphQuery for DgraphGraphStore {
    /// The nodes of a kind, of an account. The scan is the projection's
    /// own: a rebuild building apart holds nodes this projection never
    /// answers with.
    async fn nodes(&self, tenant: &Hrn, kind: NodeKind) -> Result<Vec<GraphNode>, GraphError> {
        let query = format!(
            "{{ q(func: eq(hodei.graph.node.tenant, {tenant_literal})) \
             @filter(eq(hodei.graph.node.kind, {kind_literal}) AND eq(hodei.graph.projection, {projection})) {{ {NODE_FIELDS} }} }}",
            tenant_literal = dql_string(&tenant.to_string()),
            kind_literal = dql_string(kind.as_str()),
            projection = dql_string(&self.projection),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(rows.iter().filter_map(node_of).collect())
    }

    /// The relations that start at a node, of an account: the native
    /// `hodei.graph.out` traversal.
    async fn relations_from(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        self.relations_via(tenant, node, true).await
    }

    /// The relations that end at a node, of an account: the reverse
    /// `~hodei.graph.to` traversal.
    async fn relations_to(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        self.relations_via(tenant, node, false).await
    }

    /// The lineage of an artefact: the walk back from the artefact to
    /// its builds, their revisions and the repository, and the walk
    /// forward to its deployments. The walk is the Surreal adapter's,
    /// over the native traversals.
    async fn lineage(&self, tenant: &Hrn, artifact: &Hrn) -> Result<Lineage, GraphError> {
        let artefact = self
            .node_of_kind(tenant, artifact, NodeKind::Artifact)
            .await?
            .into_iter()
            .next();
        let mut lineage = Lineage {
            artifact: artefact,
            ..Lineage::default()
        };

        // The builds that produced it: the relations that END at the
        // artefact are the ones that say who produced it.
        let produced = self.relations_to(tenant, artifact).await?;
        let evidence = produced.clone();
        for relation in &produced {
            if relation.relation == RelationType::Produced {
                lineage.builds.extend(
                    self.node_of_kind(tenant, &relation.from, NodeKind::Build)
                        .await?,
                );
            }
        }

        // The revisions those builds used, and the repository they
        // belong to, each counted once however many relations name it.
        for build in &lineage.builds {
            for relation in self.relations_to(tenant, &build.id).await? {
                let is_source = relation.relation == RelationType::UsedSource;
                let is_trigger = relation.relation == RelationType::Triggered;
                if !is_source && !is_trigger {
                    continue;
                }
                let revisions = self
                    .node_of_kind(tenant, &relation.from, NodeKind::SourceRevision)
                    .await?;
                for revision in revisions {
                    if !lineage.revisions.iter().any(|held| held.id == revision.id) {
                        lineage.revisions.push(revision);
                    }
                }
            }
        }
        for revision in &lineage.revisions {
            for relation in self.relations_to(tenant, &revision.id).await? {
                if relation.relation == RelationType::HasRevision {
                    let repositories = self
                        .node_of_kind(tenant, &relation.from, NodeKind::Repository)
                        .await?;
                    lineage.repository = repositories.into_iter().next();
                }
            }
        }

        // The deployments of the artefact: the relations that START at it.
        for relation in self.relations_from(tenant, artifact).await? {
            if relation.relation == RelationType::DeployedAs {
                lineage.deployments.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Deployment)
                        .await?,
                );
            }
        }
        lineage.evidence = evidence;
        Ok(lineage)
    }

    /// The deployments of an artefact.
    async fn deployments(
        &self,
        tenant: &Hrn,
        artifact: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut deployments = Vec::new();
        for relation in self.relations_from(tenant, artifact).await? {
            if relation.relation == RelationType::DeployedAs {
                deployments.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Deployment)
                        .await?,
                );
            }
        }
        Ok(deployments)
    }

    /// The artefacts a build produced.
    ///
    /// The produced relation goes FROM the build TO the artefact, so the
    /// walk starts at the build and follows `hodei.graph.out` forward.
    /// HSP-189 had replicated the reference walk verbatim, which read the
    /// relations that END at the build and found nothing under the
    /// projector's direction; HSP-200 corrected the walk in both ports
    /// (see the task evidence for the decision).
    async fn artifacts_of_build(
        &self,
        tenant: &Hrn,
        build: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut artifacts = Vec::new();
        for relation in self.relations_from(tenant, build).await? {
            if relation.relation == RelationType::Produced {
                artifacts.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Artifact)
                        .await?,
                );
            }
        }
        Ok(artifacts)
    }

    /// The artefacts of a repository: the walk of three hops, the same
    /// the Surreal adapter makes.
    async fn artifacts_of_repository(
        &self,
        tenant: &Hrn,
        repository: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut artifacts: Vec<GraphNode> = Vec::new();
        for relation in self.relations_from(tenant, repository).await? {
            if relation.relation != RelationType::HasRevision {
                continue;
            }
            for trigger in self.relations_from(tenant, &relation.to).await? {
                if trigger.relation != RelationType::Triggered {
                    continue;
                }
                for artifact in self.artifacts_of_build(tenant, &trigger.to).await? {
                    if !artifacts.iter().any(|held| held.id == artifact.id) {
                        artifacts.push(artifact);
                    }
                }
            }
        }
        Ok(artifacts)
    }
}

// ---------------------------------------------------------------------------
// Bounded reads (HSP-200)
// ---------------------------------------------------------------------------

/// The DQL of one hop of a bounded walk, expanded at the server.
///
/// The frontier is bound by its node keys; the hop expands the native
/// edges of those nodes with the account, the asserted status and the
/// relation types as filters ON the expansion, and reads the far end of
/// every crossed relation in the same round trip. The `first` bound
/// caps the expansion per frontier node, so the server answers a
/// bounded question and only the bounded frontier comes back — the
/// whole graph never travels.
///
/// The account rides on every hop: the relations and the far nodes are
/// both filtered by tenant, so a walk cannot leak across accounts even
/// where an edge would reach.
fn frontier_expansion_query(
    tenant: &str,
    projection: &str,
    keys: &[String],
    types: &[RelationType],
    forward: bool,
    first_per_node: usize,
) -> String {
    let forward_edge = if forward {
        "hodei.graph.out"
    } else {
        "~hodei.graph.to"
    };
    // The far end of the relation: the node the relation points at
    // (forward) or the node the relation starts at (backward).
    let far_edge = if forward {
        "hodei.graph.to"
    } else {
        "~hodei.graph.out"
    };
    let tenant_literal = dql_string(tenant);
    let projection_literal = dql_string(projection);
    let keys_literal = keys
        .iter()
        .map(|key| dql_string(key))
        .collect::<Vec<_>>()
        .join(", ");
    let mut relation_filter = format!(
        "eq(hodei.graph.rel.tenant, {tenant_literal}) AND \
         eq(hodei.graph.rel.status, {status_literal})",
        status_literal = dql_string(STATUS_ASSERTED),
    );
    if !types.is_empty() {
        let wanted = types
            .iter()
            .map(|kind| dql_string(kind.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        relation_filter.push_str(&format!(" AND eq(hodei.graph.rel.type, [{wanted}])"));
    }
    let node_filter = format!(
        "eq(hodei.graph.node.tenant, {tenant_literal}) AND \
         eq(hodei.graph.projection, {projection_literal})"
    );
    format!(
        "{{ q(func: eq(hodei.graph.node.key, [{keys_literal}])) @filter({node_filter}) {{\n\
           hodei.graph.node.key\n\
           {forward_edge} (first: {first_per_node}) @filter({relation_filter}) {{\n\
             {RELATION_FIELDS}\n\
             {far_edge} @filter({node_filter}) {{ {NODE_FIELDS} }}\n\
           }}\n\
        }} }}"
    )
}

impl DgraphGraphStore {
    /// One hop of a bounded walk, expanded at the server.
    ///
    /// Returns the relations the frontier crossed and the node records
    /// at their far end, both deduplicated and sorted (relations by
    /// identity, nodes by key), so the answer does not depend on the
    /// order the server walks its edges in.
    async fn expand_frontier(
        &self,
        tenant: &Hrn,
        frontier_keys: &[String],
        types: &[RelationType],
        forward: bool,
        first_per_node: usize,
    ) -> Result<(Vec<GraphRelation>, Vec<GraphNode>), GraphError> {
        if frontier_keys.is_empty() || first_per_node == 0 {
            return Ok((Vec::new(), Vec::new()));
        }
        let query = frontier_expansion_query(
            &tenant.to_string(),
            &self.projection,
            frontier_keys,
            types,
            forward,
            first_per_node,
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut relations: BTreeMap<String, GraphRelation> = BTreeMap::new();
        let mut far: BTreeMap<String, GraphNode> = BTreeMap::new();
        let far_edge = if forward {
            "hodei.graph.to"
        } else {
            "~hodei.graph.out"
        };
        for row in &rows {
            let crossed = row
                .get(if forward {
                    "hodei.graph.out"
                } else {
                    "~hodei.graph.to"
                })
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for relation_row in &crossed {
                if let Some(relation) = relation_of(relation_row) {
                    relations.insert(relation.identity(), relation);
                }
                // The far end travels as an object when the edge
                // points at one node (`hodei.graph.to` is a scalar uid)
                // and as an array when it points at many: both shapes
                // are one node here.
                let far_row = match relation_row.get(far_edge) {
                    Some(serde_json::Value::Array(rows)) => rows.first().cloned(),
                    Some(row @ serde_json::Value::Object(_)) => Some(row.clone()),
                    _ => None,
                };
                if let Some(node) = far_row.as_ref().and_then(node_of) {
                    far.insert(node.id.to_string(), node);
                }
            }
        }
        Ok((
            relations.into_values().collect(),
            far.into_values().collect(),
        ))
    }

    /// A node of this projection by its identity, whatever kind it is.
    async fn node_by_key(&self, tenant: &Hrn, id: &Hrn) -> Result<Option<GraphNode>, GraphError> {
        let node_key = self.node_key_scoped(&tenant.to_string(), &id.to_string());
        let node_filter = format!(
            "eq(hodei.graph.node.tenant, {}) AND eq(hodei.graph.projection, {})",
            dql_string(&tenant.to_string()),
            dql_string(&self.projection),
        );
        let query = format!(
            "{{ q(func: eq(hodei.graph.node.key, {key_literal})) @filter({node_filter}) {{ {NODE_FIELDS} }} }}",
            key_literal = dql_string(&node_key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        Ok(response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(node_of))
    }
}

#[async_trait]
impl BoundedGraphQuery for DgraphGraphStore {
    /// The lineage of an artefact under limits.
    ///
    /// The walk is the reference lineage walk, hop by hop, with every
    /// hop expanded at the server and accounted by one budget: what the
    /// walk spent and what it had to leave behind travels in the trace,
    /// and a result that hit a limit says so instead of passing for
    /// whole.
    async fn bounded_lineage(
        &self,
        tenant: &Hrn,
        artifact: &Hrn,
        limits: &GraphReadLimits,
    ) -> Result<BoundedLineage, GraphError> {
        let mut budget = WalkBudget::new(*limits);
        let mut lineage = Lineage::default();

        // The artefact itself costs the first round trip.
        budget.round_trip();
        lineage.artifact = self.node_by_key(tenant, artifact).await?;
        let artifact_key = self.node_key_scoped(&tenant.to_string(), &artifact.to_string());

        // One hop of the walk, with the shapes the lineage answers with:
        // the relations that END at the nodes say what produced and used
        // them, the ones that START at the artefact say where it is
        // deployed.
        macro_rules! hop {
            ($frontier:expr, $types:expr, $forward:expr) => {{
                if !budget.may_expand() {
                    budget.leave_frontier($frontier.len());
                    (Vec::new(), Vec::new())
                } else {
                    budget.hop();
                    budget.round_trip();
                    let first = budget.edges_remaining() + 1;
                    let (relations, far) = self
                        .expand_frontier(tenant, $frontier, $types, $forward, first)
                        .await?;
                    let keep = budget.admit_edges(relations.len());
                    let kept = &relations[..keep];
                    let admitted = budget.admit_nodes(far.len());
                    let kept_nodes: Vec<GraphNode> = far[..admitted.min(far.len())].to_vec();
                    if budget.stopped() {
                        // Whatever the frontier reached, the walk does
                        // not go on: say what was left behind.
                        budget.leave_frontier(kept_nodes.len());
                    }
                    (kept.to_vec(), kept_nodes)
                }
            }};
        }

        // Hop 1: everything asserted that ends at the artefact is
        // evidence of its lineage; the produced ones name its builds.
        let (evidence, sources) = hop!(std::slice::from_ref(&artifact_key), &[], false);
        lineage.evidence = evidence;
        let mut builds: Vec<GraphNode> = sources
            .iter()
            .filter(|node| node.kind == NodeKind::Build)
            .cloned()
            .collect();
        builds.sort_by_key(|node| node.id.to_string());

        // Hop 2: the revisions the builds used or were triggered by.
        let build_keys: Vec<String> = builds
            .iter()
            .map(|node| self.node_key_scoped(&tenant.to_string(), &node.id.to_string()))
            .collect();
        let (_, revision_candidates) = hop!(
            &build_keys,
            &[RelationType::UsedSource, RelationType::Triggered],
            false
        );
        let mut revisions: Vec<GraphNode> = revision_candidates
            .iter()
            .filter(|node| node.kind == NodeKind::SourceRevision)
            .cloned()
            .collect();
        revisions.sort_by_key(|node| node.id.to_string());

        // Hop 3: the repository the revisions belong to.
        let revision_keys: Vec<String> = revisions
            .iter()
            .map(|node| self.node_key_scoped(&tenant.to_string(), &node.id.to_string()))
            .collect();
        let (_, repositories) = hop!(&revision_keys, &[RelationType::HasRevision], false);
        lineage.repository = repositories
            .iter()
            .find(|node| node.kind == NodeKind::Repository)
            .cloned();

        // Hop 4: where the artefact is deployed.
        let (_, deployments) = hop!(
            std::slice::from_ref(&artifact_key),
            &[RelationType::DeployedAs],
            true
        );
        lineage.deployments = deployments
            .iter()
            .filter(|node| node.kind == NodeKind::Deployment)
            .cloned()
            .collect();

        // The shapes the reference lineage answers with, whatever the
        // budget let the walk keep.
        lineage.builds = builds;
        lineage.revisions = revisions;
        Ok(BoundedLineage {
            lineage,
            trace: budget.trace(),
        })
    }

    /// The dependency graph that starts at a node under limits: the
    /// walk follows the asserted relations forward, hop by hop, at the
    /// server, and never visits a node twice — a cycle in the graph
    /// neither hangs the walk nor duplicates a node in the answer.
    async fn bounded_dependencies(
        &self,
        tenant: &Hrn,
        node: &Hrn,
        limits: &GraphReadLimits,
    ) -> Result<BoundedTraversal, GraphError> {
        let mut budget = WalkBudget::new(*limits);
        let mut out = BoundedTraversal::default();

        budget.round_trip();
        out.root = self.node_by_key(tenant, node).await?;
        let root_key = self.node_key_scoped(&tenant.to_string(), &node.to_string());
        // The visited set is the domain of node HRNs, the same domain
        // the far nodes are deduplicated in.
        let mut visited: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        visited.insert(node.to_string());
        let mut frontier = vec![root_key];

        while !frontier.is_empty() {
            if !budget.may_expand() {
                budget.leave_frontier(frontier.len());
                break;
            }
            budget.hop();
            budget.round_trip();
            // One more than the budget allows: when the server comes
            // back with it, the answer had edges the budget does not,
            // and the trace says so.
            let first = budget.edges_remaining() + 1;
            let (relations, far) = self
                .expand_frontier(tenant, &frontier, &[], true, first)
                .await?;
            let keep = budget.admit_edges(relations.len());
            out.edges.extend(relations[..keep].to_vec());

            // The nodes the walk has not met yet, in the order the
            // answer carries them. Whatever the walk cannot keep or go
            // on with is left behind, and the trace says so.
            let mut fresh: Vec<GraphNode> = Vec::new();
            for candidate in &far {
                let key = candidate.id.to_string();
                if !visited.contains(&key) && !fresh.iter().any(|held| held.id.to_string() == key) {
                    fresh.push(candidate.clone());
                }
            }
            let admitted = budget.admit_nodes(fresh.len()).min(fresh.len());
            let mut next_frontier = Vec::new();
            for reached in fresh[..admitted].iter() {
                visited.insert(reached.id.to_string());
                out.nodes.push(reached.clone());
                next_frontier
                    .push(self.node_key_scoped(&tenant.to_string(), &reached.id.to_string()));
            }
            if budget.stopped() {
                budget.leave_frontier(fresh.len());
                break;
            }
            frontier = next_frontier;
        }
        out.trace = budget.trace();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrn(text: &str) -> Hrn {
        Hrn::from_string(text).expect("a valid HRN")
    }

    fn tenant() -> Hrn {
        hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/hsp189")
    }

    fn node(id: &str, kind: NodeKind) -> GraphNode {
        GraphNode::new(hrn(id), tenant(), kind, chrono::Utc::now())
    }

    fn relation(from: &str, kind: RelationType, to: &str, evidence: &str) -> GraphRelation {
        GraphRelation::new(
            kind,
            hrn(from),
            hrn(to),
            tenant(),
            EvidenceId::new(evidence.to_owned()),
            chrono::Utc::now(),
        )
    }

    /// The identity keys are deterministic and scoped: the same node in
    /// two accounts is two keys; two evidences on one relation share the
    /// relation key and differ in the support key.
    #[test]
    fn the_identity_keys_are_stable_and_scoped() {
        let key = DgraphGraphStore::node_key("hrn:hodei:artifacts:eu-west-1:acme", "hrn:x");
        assert_eq!(key, "hrn:hodei:artifacts:eu-west-1:acme|hrn:x");
        assert_ne!(
            key,
            DgraphGraphStore::node_key("hrn:hodei:artifacts:eu-west-1:other", "hrn:x"),
            "two tenants never share a node key"
        );
        let identity = "produced|hrn:build|hrn:artifact";
        let rel = DgraphGraphStore::relation_key("acme", identity);
        assert_eq!(DgraphGraphStore::relation_key("acme", identity), rel);
        assert_ne!(rel, DgraphGraphStore::relation_key("other", identity));
        let sup = DgraphGraphStore::support_key("acme", identity, "ev-1");
        assert_eq!(DgraphGraphStore::support_key("acme", identity, "ev-1"), sup);
        assert_ne!(sup, DgraphGraphStore::support_key("acme", identity, "ev-2"));
        assert_ne!(sup, rel);
    }

    /// The changeset fingerprint is deterministic: the same changeset
    /// twice, the same fingerprint; another evidence, another
    /// fingerprint.
    #[test]
    fn the_changeset_fingerprint_is_deterministic() {
        let changes = GraphChangeSet::empty()
            .with_node(node(
                "hrn:hodei:artifacts:eu-west-1:acme:Build/b1",
                NodeKind::Build,
            ))
            .with_relation(relation(
                "hrn:hodei:artifacts:eu-west-1:acme:Build/b1",
                RelationType::Produced,
                "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1",
                "ev-1",
            ))
            .with_cause("cause-1");
        let first = DgraphGraphStore::changeset_fingerprint(&changes);
        assert_eq!(
            first,
            DgraphGraphStore::changeset_fingerprint(&changes.clone())
        );
        let other = changes.clone().with_relation(relation(
            "hrn:hodei:artifacts:eu-west-1:acme:Build/b1",
            RelationType::Produced,
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1",
            "ev-2",
        ));
        assert_ne!(first, DgraphGraphStore::changeset_fingerprint(&other));
    }

    /// Two evidences supporting one relation produce ONE relation node
    /// in the transaction: the query binds the relation once, by its
    /// identity, and the two supports hang from it.
    #[test]
    fn two_evidences_are_one_relation_with_two_supports() {
        let from = "hrn:hodei:artifacts:eu-west-1:acme:Build/b1";
        let to = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1";
        let changes = GraphChangeSet::empty()
            .with_node(node(from, NodeKind::Build))
            .with_node(node(to, NodeKind::Artifact))
            .with_relation(relation(from, RelationType::Produced, to, "ev-1"))
            .with_relation(relation(from, RelationType::Produced, to, "ev-2"));
        let transaction =
            DgraphGraphStore::changeset_upsert_scoped(&changes, "", ACTIVE_PROJECTION);
        assert_eq!(
            transaction
                .query
                .matches("as var(func: eq(hodei.graph.rel.key")
                .count(),
            1,
            "the relation is bound once: {}",
            transaction.query
        );
        assert_eq!(
            transaction
                .query
                .matches("as var(func: eq(hodei.graph.support.key")
                .count(),
            2,
            "two supports hang from the one relation: {}",
            transaction.query
        );
        let blocks: String = transaction
            .blocks
            .iter()
            .map(|block| format!("{}\n{}", block.set_nquads, block.delete_nquads))
            .collect();
        assert_eq!(
            blocks.matches("<hodei.graph.support.key>").count(),
            2,
            "two support records are written: {blocks}"
        );
    }

    /// The withdrawal of one support of a two-support relation carries
    /// the decision blocks: what remains decides between asserted and
    /// UNKNOWN, and the edges are retired only in the UNKNOWN branch.
    #[test]
    fn the_withdrawal_decides_by_what_remains() {
        let from = "hrn:hodei:artifacts:eu-west-1:acme:Build/b1";
        let to = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1";
        let now = chrono::Utc::now();
        let changes = GraphChangeSet::empty().with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Produced,
            hrn(from),
            hrn(to),
            tenant(),
            EvidenceId::new("ev-1".to_owned()),
            now,
        ));
        let transaction =
            DgraphGraphStore::changeset_upsert_scoped(&changes, "", ACTIVE_PROJECTION);
        let blocks: String = transaction
            .blocks
            .iter()
            .map(|block| {
                format!(
                    "{}|{}|{}",
                    block.cond.as_deref().unwrap_or(""),
                    block.set_nquads,
                    block.delete_nquads
                )
            })
            .collect();
        assert!(
            blocks.contains("eq(len(wleft0), 0)"),
            "the UNKNOWN branch is gated on nothing remaining: {blocks}"
        );
        assert!(
            blocks.contains("gt(len(wleft0), 0)"),
            "the asserted branch is gated on something remaining: {blocks}"
        );
        assert!(
            blocks.contains(&format!(
                "<hodei.graph.rel.status> {}",
                dql_string(STATUS_UNKNOWN)
            )),
            "the UNKNOWN branch writes the unknown status: {blocks}"
        );
        assert!(
            blocks.contains("<hodei.graph.to> * ."),
            "the UNKNOWN branch retires the native edge: {blocks}"
        );
    }

    /// A changeset that both re-asserts a relation and withdraws one of
    /// its supports carries NO recomputation blocks: the add blocks
    /// already wrote `asserted`, and the client-folded remaining count
    /// (the support added here is not visible to the query) is what
    /// keeps the answer right.
    #[test]
    fn a_relation_that_gains_supports_here_is_not_recomputed() {
        let from = "hrn:hodei:artifacts:eu-west-1:acme:Build/b1";
        let to = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1";
        let now = chrono::Utc::now();
        let changes = GraphChangeSet::empty()
            .with_relation(relation(from, RelationType::Produced, to, "ev-2"))
            .with_withdrawal(GraphRelationWithdrawal::new(
                RelationType::Produced,
                hrn(from),
                hrn(to),
                tenant(),
                EvidenceId::new("ev-1".to_owned()),
                now,
            ));
        let transaction =
            DgraphGraphStore::changeset_upsert_scoped(&changes, "", ACTIVE_PROJECTION);
        let blocks: String = transaction
            .blocks
            .iter()
            .map(|block| block.cond.as_deref().unwrap_or("").to_string())
            .collect();
        assert!(
            !blocks.contains("wleft"),
            "no recomputation blocks when the relation gains supports here: {blocks}"
        );
    }

    /// The native edges are written in presence branches: for a relation
    /// whose ends are co-created by the same changeset, the branch the
    /// server picks references the blank labels; for ends that predate
    /// it, the uid variables.
    #[test]
    fn the_edges_are_written_per_presence_branch() {
        let from = "hrn:hodei:artifacts:eu-west-1:acme:Build/b1";
        let to = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/a1";
        let changes = GraphChangeSet::empty()
            .with_node(node(from, NodeKind::Build))
            .with_node(node(to, NodeKind::Artifact))
            .with_relation(relation(from, RelationType::Produced, to, "ev-1"));
        let transaction =
            DgraphGraphStore::changeset_upsert_scoped(&changes, "", ACTIVE_PROJECTION);
        let blocks: String = transaction
            .blocks
            .iter()
            .map(|block| {
                format!(
                    "[{}]\n{}",
                    block.cond.as_deref().unwrap_or(""),
                    block.set_nquads
                )
            })
            .collect();
        // Both presence combinations of the ends exist as branches.
        assert!(
            blocks.contains("eq(len(n0), 1)"),
            "the pre-existing branch: {blocks}"
        );
        assert!(
            blocks.contains("eq(len(n0), 0)"),
            "the co-created branch: {blocks}"
        );
        assert!(
            blocks.contains("_:n0 <hodei.graph.out> _:r0 ."),
            "the co-created ends are linked through blank labels: {blocks}"
        );
        assert!(
            blocks.contains("uid(n0) <hodei.graph.out> _:r0 ."),
            "a pre-existing source is linked by uid to the co-created relation: {blocks}"
        );
    }
}
