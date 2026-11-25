//! The impact view of the risk, materialized in the SAME Dgraph the
//! walk reads.
//!
//! This is the Dgraph port of [`crate::impact_view`]: the same
//! observable contract — the view says what the authoritative walk
//! says, a delta invalidates ONLY the advisories whose route it
//! touches, the watermark exposes the lag and never dresses a stale
//! answer as current — mapped onto the engine the stack already proves,
//! so risk and reachability read ONE graph and the deployment runs no
//! second database.
//!
//! # The pieces
//!
//! - **The walk** ([`walk_impact_dgraph`]) is the reference traversal of
//!   HSP-82/83, level by level, over the native edges HSP-189 wrote:
//!   the components the advisory is about (reverse of `AffectedBy`),
//!   the artifacts that hold them (reverse of `Contains`), the
//!   components that depend on them (reverse of `DependsOn`), and the
//!   deployments of the artifacts. Every hop is the bounded expansion
//!   of HSP-200 ([`DgraphGraphStore::expand_frontier`]) with the
//!   account and the asserted status as filters, and the whole walk
//!   accounts itself in a [`TraversalTrace`]: a walk that hit a bound
//!   says so, and the view REFUSES to materialize a partial answer.
//! - **The view** ([`DgraphImpactView`]) is ONE route node per
//!   (tenant, advisory) in the same graph: the subjects with their
//!   depths, the path that explains each one, and the route of support
//!   the walk used — relation identities, evidences and node endpoints,
//!   exact-indexed — so the invalidation of a delta is a lookup and
//!   never a scan.
//! - **The deltas** come from the projection frontier of HSP-199: the
//!   same transaction that applies a changeset says its deltas and
//!   advances the checkpoint. The view consumes them
//!   ([`DgraphImpactView::apply_delta`]) and rebuilds ONLY the
//!   advisories the delta touches; the watermark of each route is the
//!   delta version it is complete up to, and the consumer's own
//!   watermark ([`DgraphImpactView::consumer_watermark`]) is explicit
//!   state in the engine — a restart resumes where it said it was, and
//!   the lag is never presented as currency.
//!
//! # Fail-closed
//!
//! A store that fails answers with an error, never with an answer that
//! looks like zero impact: every read failure surfaces as
//! [`ImpactReadError::Unavailable`], a route whose walk was truncated
//! is never materialized, and a relation whose last support was
//! withdrawn is not walked at all — the view forgets the subject the
//! same moment the graph stops asserting it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use hodei_evidence::application::projection::{
    ProjectionDelivery, ProjectionFrontier, ProjectionReceipt,
};
use hodei_evidence::domain::delta::RelationDelta;
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::ports::graph::{GraphReadLimits, TraversalTrace, WalkBudget};
use hodei_evidence::{GraphChangeSet, GraphRelation};
use hodei_evidence_storage::dgraph::frontier::DgraphProjectionFrontier;
use hodei_evidence_storage::dgraph::graph::DgraphGraphStore;
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    dql_string,
};
use kernel::Hrn;

use crate::RiskError;
use crate::impact_view::{
    Freshness, ImpactAnswer, ImpactReadError, ImpactReader, ImpactRoute, ImpactRow, ImpactSnapshot,
    SubjectPath,
};

/// The type tag of the impact route nodes in the graph.
pub const IMPACT_ROUTE_TYPE: &str = "HodeiImpactRoute";

/// How many times a rebuild may lose the write race before giving up.
/// The bound is the one the changeset writer runs under.
const MAX_ATTEMPTS: usize = 8;

/// The fields every route node is read with.
const ROUTE_FIELDS: &str = "hodei.impact.key hodei.impact.tenant hodei.impact.advisory \
     hodei.impact.route_relation hodei.impact.route_evidence hodei.impact.route_node \
     hodei.impact.subjects hodei.impact.paths hodei.impact.deployments \
     hodei.impact.watermark hodei.impact.updated_at";

/// What one bounded walk of the impact saw, with the receipt of how it
/// was resolved: the subjects the advisory reaches, their depths, the
/// routes of support that reach them and the trace of the bounds the
/// walk ran under.
#[derive(Debug, Default)]
pub struct DgraphImpactWalk {
    /// The components the vulnerability is in, at depth zero.
    pub affected_components: BTreeSet<String>,
    /// The artifacts that hold them, with their depth.
    pub artifacts: BTreeMap<String, usize>,
    /// The deployments that run those artifacts.
    pub deployments: BTreeSet<String>,
    /// The identities of the relations the walk used.
    pub route_relations: BTreeSet<String>,
    /// The evidences that support those relations.
    pub route_evidence: BTreeSet<String>,
    /// The nodes the walk traversed: the endpoints of everything it used.
    pub route_nodes: BTreeSet<String>,
    /// The edges that discovered each subject: (relation identity,
    /// evidence id), the path that explains it.
    pub subject_routes: BTreeMap<String, Vec<(String, String)>>,
    /// How the walk was resolved: the limits it ran under, what it spent
    /// and what, if anything, it had to leave behind.
    pub trace: TraversalTrace,
}

impl DgraphImpactWalk {
    /// The subjects of the answer, from the walk: the artifacts with
    /// their depth, the components the vulnerability is in at depth
    /// zero, each with the path that explains it.
    pub fn answer_subjects(&self) -> Vec<SubjectPath> {
        let mut subjects: Vec<SubjectPath> = Vec::new();
        for (subject, depth) in &self.artifacts {
            let depth = if *depth == usize::MAX { 1 } else { *depth };
            let edges = self.subject_routes.get(subject);
            subjects.push(SubjectPath {
                subject: subject.clone(),
                depth,
                relations: edges
                    .map(|edges| edges.iter().map(|(relation, _)| relation.clone()).collect())
                    .unwrap_or_default(),
                evidences: edges
                    .map(|edges| edges.iter().map(|(_, evidence)| evidence.clone()).collect())
                    .unwrap_or_default(),
            });
        }
        for component in &self.affected_components {
            let edges = self.subject_routes.get(component);
            subjects.push(SubjectPath {
                subject: component.clone(),
                depth: 0,
                relations: edges
                    .map(|edges| edges.iter().map(|(relation, _)| relation.clone()).collect())
                    .unwrap_or_default(),
                evidences: edges
                    .map(|edges| edges.iter().map(|(_, evidence)| evidence.clone()).collect())
                    .unwrap_or_default(),
            });
        }
        subjects.sort_by(|left, right| left.subject.cmp(&right.subject));
        subjects
    }

    /// The subjects as the snapshot carries them: (subject, depth),
    /// sorted, the same shape the reference walk answers with.
    pub fn snapshot_subjects(&self) -> Vec<(String, usize)> {
        let mut subjects: Vec<(String, usize)> = self
            .artifacts
            .iter()
            .map(|(subject, depth)| {
                let depth = if *depth == usize::MAX { 1 } else { *depth };
                (subject.clone(), depth)
            })
            .collect();
        for component in &self.affected_components {
            subjects.push((component.clone(), 0));
        }
        subjects.sort_by(|left, right| left.0.cmp(&right.0));
        subjects
    }
}

/// Records what one hop used as it uses it: the identity of the
/// relation, the evidence that supports it and the nodes it joins, for
/// the route of the advisory and for the ones that invalidate.
fn record_route(
    route_relations: &mut BTreeSet<String>,
    route_evidence: &mut BTreeSet<String>,
    route_nodes: &mut BTreeSet<String>,
    anchors: &[String],
    rows: &[GraphRelation],
) {
    for anchor in anchors {
        route_nodes.insert(anchor.clone());
    }
    for row in rows {
        route_relations.insert(row.identity());
        route_evidence.insert(row.evidence_id.as_str().to_owned());
        route_nodes.insert(row.from.to_string());
    }
}

/// The walk of the impact over the native graph, under limits: the same
/// traversal, the same cost and the same route the reference view
/// records, with every hop expanded at the server and accounted in one
/// budget.
///
/// The walk goes UP: the dependency is the reverse of the relation. The
/// nodes of a level are asked about TOGETHER — one expansion per level,
/// so the cost of a walk of several hops is the cost of the round trips
/// and not of the nodes — and the level of a node IS how far the
/// dependency is: the component the vulnerability is in is at distance
/// zero, and the artifact that holds it at distance one.
pub async fn walk_impact_dgraph(
    graph: &DgraphGraphStore,
    tenant: &Hrn,
    vulnerability: &Hrn,
    limits: &GraphReadLimits,
) -> Result<DgraphImpactWalk, RiskError> {
    let mut budget = WalkBudget::new(*limits);
    let mut walk = DgraphImpactWalk::default();

    // The components the vulnerability is about: one round trip, the
    // reverse of the AffectedBy edges that point at the advisory.
    let affected_rows = expand(
        &mut budget,
        graph,
        tenant,
        &[vulnerability.to_string()],
        &[RelationType::AffectedBy],
        false,
    )
    .await?;
    let mut affected: Vec<Hrn> = Vec::new();
    for row in &affected_rows {
        if let Some(hrn) = Hrn::from_string(&row.from.to_string()) {
            affected.push(hrn);
        }
        walk.subject_routes
            .entry(row.from.to_string())
            .or_default()
            .push((row.identity(), row.evidence_id.as_str().to_owned()));
    }
    record_route(
        &mut walk.route_relations,
        &mut walk.route_evidence,
        &mut walk.route_nodes,
        &[vulnerability.to_string()],
        &affected_rows,
    );
    for component in &affected {
        walk.affected_components.insert(component.to_string());
    }

    // The walk goes up, the same way the authoritative answer goes: the
    // nodes of a level are asked about TOGETHER.
    let mut containers: BTreeMap<String, Vec<Hrn>> = BTreeMap::new();
    let mut levels: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut level: Vec<Hrn> = affected.clone();
    let mut depth = 0usize;
    for node in &affected {
        levels.insert(node.to_string(), 0);
    }

    while !level.is_empty() {
        if !budget.may_expand() {
            budget.leave_frontier(level.len());
            break;
        }
        budget.hop();
        let asked: Vec<String> = level.iter().map(|node| node.to_string()).collect();
        let rows = expand(
            &mut budget,
            graph,
            tenant,
            &asked,
            &[RelationType::Contains, RelationType::DependsOn],
            false,
        )
        .await?;
        record_route(
            &mut walk.route_relations,
            &mut walk.route_evidence,
            &mut walk.route_nodes,
            &asked,
            &rows,
        );

        let mut containers_of: Vec<Hrn> = Vec::new();
        let mut next_level: Vec<Hrn> = Vec::new();
        for row in &rows {
            walk.subject_routes
                .entry(row.from.to_string())
                .or_default()
                .push((row.identity(), row.evidence_id.as_str().to_owned()));
            if row.relation == RelationType::Contains {
                containers_of.push(row.from.clone());
            }
        }
        for row in &rows {
            if row.relation == RelationType::DependsOn {
                let key = row.from.to_string();
                if seen.insert(key.clone()) {
                    levels.insert(key, depth + 1);
                    next_level.push(row.from.clone());
                }
            }
        }
        for node in &level {
            let owner = node.to_string();
            for container in &containers_of {
                if container == node {
                    continue;
                }
                containers
                    .entry(owner.clone())
                    .or_default()
                    .push(container.clone());
            }
        }

        level = next_level;
        depth += 1;
    }

    // The deployments of the artifacts that hold them, and the route
    // they take — the same reading the reference walk makes.
    let mut artefact_ids: BTreeSet<String> = BTreeSet::new();
    for artefact in containers.values().flatten() {
        artefact_ids.insert(artefact.to_string());
    }
    // The depth of an artifact: the level of the component that holds it
    // plus one, taken over EVERY component that holds it, the same way
    // the authoritative answer says it.
    for artefact in &artefact_ids {
        walk.artifacts.insert(artefact.clone(), usize::MAX);
    }
    for (component, owners) in &containers {
        let component_level = levels.get(component).copied().unwrap_or(0);
        for owner in owners {
            if let Some(entry) = walk.artifacts.get_mut(&owner.to_string()) {
                *entry = (*entry).min(component_level + 1);
            }
        }
    }
    if !artefact_ids.is_empty() {
        let deployed: Vec<String> = artefact_ids.iter().cloned().collect();
        let deployment_rows = expand(
            &mut budget,
            graph,
            tenant,
            &deployed,
            &[RelationType::DeployedAs],
            true,
        )
        .await?;
        record_route(
            &mut walk.route_relations,
            &mut walk.route_evidence,
            &mut walk.route_nodes,
            &deployed,
            &deployment_rows,
        );
        for row in &deployment_rows {
            walk.deployments.insert(row.from.to_string());
            walk.subject_routes
                .entry(row.from.to_string())
                .or_default()
                .push((row.identity(), row.evidence_id.as_str().to_owned()));
        }
    }

    walk.trace = budget.trace();
    Ok(walk)
}

/// The blast radius of a vulnerability over the Dgraph graph: the Dgraph
/// counterpart of [`crate::blast_radius`], with the same shape and the
/// same semantics.
///
/// It reads through [`walk_impact_dgraph`], so the walk is the bounded
/// one: every hop is expanded at the server with the account and the
/// asserted status as filters, and it accounts itself against `limits`.
/// The difference with the reference is the fail-closed contract of the
/// native engine: a walk that hit a bound is NOT a blast radius, and it
/// is refused with an error instead of being returned as complete.
pub async fn blast_radius_dgraph(
    graph: &DgraphGraphStore,
    tenant: &Hrn,
    vulnerability: &Hrn,
    limits: &GraphReadLimits,
) -> Result<hodei_risk::case::BlastRadius, RiskError> {
    use hodei_risk::case::{BlastRadius, SubjectDepth};

    let walk = walk_impact_dgraph(graph, tenant, vulnerability, limits).await?;
    if walk.trace.incomplete {
        return Err(RiskError::Unavailable(format!(
            "the blast radius walk hit its bounds ({:?}): a partial radius is not an answer",
            walk.trace.reasons
        )));
    }

    let affected_deployments: Vec<String> = walk.deployments.iter().cloned().collect();
    // The artifacts come out of the map already sorted by subject.
    let depths: Vec<SubjectDepth> = walk
        .artifacts
        .iter()
        .map(|(subject, depth)| SubjectDepth {
            subject: subject.clone(),
            depth: if *depth == usize::MAX { 1 } else { *depth },
            deployments: affected_deployments.clone(),
        })
        .collect();

    Ok(BlastRadius {
        affected_subjects: walk.artifacts.keys().cloned().collect(),
        affected_deployments,
        affected_components: walk.affected_components.iter().cloned().collect(),
        depths,
    })
}

/// One hop of the walk with its budget accounting: the round trip, the
/// edges received and the far nodes received. The frontier is named by
/// its node HRNs, and the expansion is bounded by what the budget still
/// allows — one more than that, so a server that comes back with it
/// says the answer was bigger than the budget, and the trace says so.
async fn expand(
    budget: &mut WalkBudget,
    graph: &DgraphGraphStore,
    tenant: &Hrn,
    frontier_hrns: &[String],
    types: &[RelationType],
    forward: bool,
) -> Result<Vec<GraphRelation>, RiskError> {
    budget.round_trip();
    let keys: Vec<String> = frontier_hrns
        .iter()
        .map(|node| node_key_of(tenant, node))
        .collect();
    let first = budget.edges_remaining() + 1;
    let (relations, far) = graph
        .expand_frontier(tenant, &keys, types, forward, first)
        .await
        .map_err(|error| RiskError::Unavailable(error.to_string()))?;
    budget.admit_edges(relations.len());
    budget.admit_nodes(far.len());
    Ok(relations)
}

/// The node key of the ACTIVE projection: the bare identity key the
/// graph store gives every node of the projection in use.
fn node_key_of(tenant: &Hrn, node: &str) -> String {
    DgraphGraphStore::node_key(&tenant.to_string(), node)
}

/// The materialized view of the impact, over the SAME graph the walk
/// reads: no second database, no second engine. The deltas come from
/// the projection frontier, and the watermark of each route is the
/// delta version it is complete up to.
pub struct DgraphImpactView {
    client: Arc<GraphClient>,
    graph: Arc<DgraphGraphStore>,
    frontier: Arc<DgraphProjectionFrontier>,
    limits: GraphReadLimits,
}

impl DgraphImpactView {
    /// The view over a connected client, the graph store and the
    /// frontier of the same tenant namespace. The maintenance walk runs
    /// under the probe bounds by default: a walk that hits a bound
    /// refuses to materialize, and the operator that wants tighter
    /// bounds names them with [`DgraphImpactView::with_limits`].
    pub fn new(
        client: Arc<GraphClient>,
        graph: Arc<DgraphGraphStore>,
        frontier: Arc<DgraphProjectionFrontier>,
    ) -> Self {
        Self {
            client,
            graph,
            frontier,
            limits: GraphReadLimits::probe(),
        }
    }

    /// The view with explicit bounds for its maintenance walks. A walk
    /// that hits one of them is NOT materialized: a partial answer is
    /// never dressed as complete.
    pub fn with_limits(mut self, limits: GraphReadLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The bounds the maintenance walks run under.
    pub fn limits(&self) -> &GraphReadLimits {
        &self.limits
    }

    /// The identity key of one route.
    pub fn route_key(tenant: &str, advisory: &str) -> String {
        format!("{tenant}|{advisory}")
    }

    /// The version the tenant's delta story has reached: the COUNT of
    /// deltas the frontier has said for the tenant.
    ///
    /// The count, and not the biggest sequence: the frontier assigns
    /// each delta the slot its delivery planned (`position+1+i`), so a
    /// delta said later can carry a sequence an older delta already
    /// held, and a maximum would stay flat while a NEW delta was said —
    /// a lag the head could never see. The ledger of deltas only
    /// GROWS (the writes are deduplicated by key, never deleted), so
    /// the count is a cut of the story that only moves forward: a
    /// watermark is complete up to it or behind it, never both.
    pub async fn said_version(&self, tenant: &Hrn) -> Result<u64, String> {
        let query = format!(
            "{{ q(func: eq(hodei.delta.tenant, {})) {{ total: count(uid) }} }}",
            dql_string(&tenant.to_string()),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| error.to_string())?;
        response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("total"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "the delta count did not answer".to_owned())
    }

    /// Materializes the impact of ONE advisory, from the bounded walk,
    /// at the version the frontier's ledger has reached. It is
    /// IDEMPOTENT: recomputing the same leaves the same, and a walk
    /// that hit a bound is refused — a partial answer is never
    /// materialized as complete.
    pub async fn rebuild_advisory(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactSnapshot, String> {
        let walk = walk_impact_dgraph(self.graph.as_ref(), tenant, advisory, &self.limits)
            .await
            .map_err(|error| error.to_string())?;
        if walk.trace.incomplete {
            return Err(format!(
                "the impact walk hit its bounds ({:?}) and the view refuses to materialize a partial answer: raise the limits or accept the refusal",
                walk.trace.reasons
            ));
        }
        let watermark = self.said_version(tenant).await?;

        // The route: the relations the walk used, the evidence that
        // supports each, and the path that explains each subject.
        let paths = walk.answer_subjects();
        let subjects = walk.snapshot_subjects();
        let route = ImpactRoute {
            tenant: tenant.to_string(),
            advisory: advisory.to_string(),
            relations: walk.route_relations.iter().cloned().collect(),
            evidences: walk.route_evidence.iter().cloned().collect(),
            nodes: walk.route_nodes.iter().cloned().collect(),
            paths: paths.clone(),
            deployments: walk.deployments.iter().cloned().collect(),
            watermark,
            updated_at: chrono::Utc::now(),
        };
        self.write_route(&route, &subjects).await?;
        tracing::debug!(
            tenant = %tenant,
            advisory = %advisory,
            subjects = subjects.len(),
            round_trips = walk.trace.round_trips,
            "the advisory view was rebuilt"
        );

        Ok(ImpactSnapshot {
            advisory: advisory.to_string(),
            subjects,
            watermark,
        })
    }

    /// Writes ONE route node, whole or not at all: the previous content
    /// of the advisory's view is replaced in the same transaction that
    /// writes the new one, so a reader never meets half of a rebuild.
    /// Two concurrent rebuilds meet on the `@upsert` of the key and the
    /// loser retries on top of whatever committed.
    async fn write_route(
        &self,
        route: &ImpactRoute,
        subjects: &[(String, usize)],
    ) -> Result<(), String> {
        let key = Self::route_key(&route.tenant, &route.advisory);
        let key_literal = dql_string(&key);
        let query = format!("{{\n  route as var(func: eq(hodei.impact.key, {key_literal}))\n}}\n");

        // The rebuild REPLACES the view of the advisory: what the new
        // walk no longer reaches is deleted, not inherited. The update
        // branch empties the node and refills it; the create branch
        // writes the same content from a blank node. Blank labels
        // unify within one transaction, so each branch is internally
        // consistent whichever the server picks.
        let content = route_nquads(route, subjects, "route");
        let fresh = route_nquads(route, subjects, "fresh");
        let blocks = vec![
            GraphMutationBlock {
                cond: Some("eq(len(route), 1)".to_owned()),
                set_nquads: String::new(),
                delete_nquads: "uid(route) * * .\n".to_owned(),
            },
            GraphMutationBlock {
                cond: Some("eq(len(route), 1)".to_owned()),
                set_nquads: content,
                delete_nquads: String::new(),
            },
            GraphMutationBlock {
                cond: Some("eq(len(route), 0)".to_owned()),
                set_nquads: fresh,
                delete_nquads: String::new(),
            },
        ];

        for attempt in 0..MAX_ATTEMPTS {
            match self
                .client
                .upsert(GraphUpsertTransaction {
                    query: query.clone(),
                    blocks: blocks.clone(),
                    commit_now: true,
                })
                .await
            {
                Ok(_) => return Ok(()),
                Err(GraphClientError::UpsertConflict { detail }) if attempt + 1 < MAX_ATTEMPTS => {
                    tracing::debug!(attempt, %detail, "the route write conflicted; revalidating");
                    tokio::time::sleep(std::time::Duration::from_millis(
                        10u64 << attempt.min(6) as u32,
                    ))
                    .await;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Err(format!(
            "the route write did not commit after {MAX_ATTEMPTS} attempts"
        ))
    }

    /// What a delta invalidates: the advisories whose routes used the
    /// relation the delta talks about, the evidence it moved, or the
    /// endpoints it joins — ONE indexed round trip, four root blocks,
    /// the account as the filter on each. SOLO las claves afectadas.
    pub async fn invalidated_by(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String> {
        let tenant = dql_string(&delta.tenant.to_string());
        let relation = dql_string(&delta.identity());
        let evidence = dql_string(delta.evidence.as_str());
        let from = dql_string(&delta.from.to_string());
        let to = dql_string(&delta.to.to_string());
        let query = format!(
            "{{\n\
             q1(func: eq(hodei.impact.route_relation, {relation})) @filter(eq(hodei.impact.tenant, {tenant})) {{ hodei.impact.advisory }}\n\
             q2(func: eq(hodei.impact.route_evidence, {evidence})) @filter(eq(hodei.impact.tenant, {tenant})) {{ hodei.impact.advisory }}\n\
             q3(func: eq(hodei.impact.route_node, {from})) @filter(eq(hodei.impact.tenant, {tenant})) {{ hodei.impact.advisory }}\n\
             q4(func: eq(hodei.impact.route_node, {to})) @filter(eq(hodei.impact.tenant, {tenant})) {{ hodei.impact.advisory }}\n\
             }}"
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| error.to_string())?;
        let mut advisories: Vec<String> = Vec::new();
        for block in ["q1", "q2", "q3", "q4"] {
            for row in response
                .json
                .get(block)
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if let Some(advisory) = row
                    .get("hodei.impact.advisory")
                    .and_then(serde_json::Value::as_str)
                {
                    advisories.push(advisory.to_owned());
                }
            }
        }
        advisories.sort();
        advisories.dedup();
        Ok(advisories
            .iter()
            .filter_map(|advisory| Hrn::from_string(advisory))
            .collect())
    }

    /// Applies a delta to the view: recomputes ONLY the advisories the
    /// delta touches, and answers with them. A re-delivery of the same
    /// delta is the same work once — the frontier deduplicates by
    /// cause, and recomputing the same leaves the same.
    pub async fn apply_delta(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String> {
        let advisories = self.invalidated_by(delta).await?;
        for advisory in &advisories {
            self.rebuild_advisory(&delta.tenant, advisory).await?;
        }
        Ok(advisories)
    }

    /// The route of one advisory, read back whole. `None` when the view
    /// has none: the absence is a response, not an error.
    async fn stored_route(
        &self,
        tenant: &Hrn,
        advisory: &str,
    ) -> Result<Option<StoredRoute>, String> {
        let query = format!(
            "{{ q(func: eq(hodei.impact.key, {})) {{ {ROUTE_FIELDS} }} }}",
            dql_string(&Self::route_key(&tenant.to_string(), advisory)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| error.to_string())?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match rows.first() {
            None => Ok(None),
            Some(row) => Ok(Some(stored_route_of(row, tenant, advisory)?)),
        }
    }

    /// The view of one advisory, with the version it is complete up to.
    /// The reader that needs the truth of the moment compares the
    /// watermark with the ledger's version: the lag is exposed, never
    /// hidden.
    pub async fn read(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<Option<ImpactSnapshot>, String> {
        let Some(stored) = self.stored_route(tenant, &advisory.to_string()).await? else {
            return Ok(None);
        };
        let mut subjects = stored.subjects;
        subjects.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Some(ImpactSnapshot {
            advisory: advisory.to_string(),
            subjects,
            watermark: stored.route.watermark,
        }))
    }

    /// The advisories the materialized view says reach ONE subject, as
    /// rows of the view.
    ///
    /// The lookup is ONE indexed round trip, not a scan: the root is
    /// the exact index of `hodei.impact.route_node` (every route
    /// records the nodes its walk traversed, which include every
    /// subject it reached), and the tenant is a filter of the SAME
    /// query. The cost is the routes that touch the subject, never the
    /// routes of the account. The index is a candidate selector only:
    /// each candidate is confirmed against the subjects it stored, so
    /// a subject that was merely traversed is not reported as reached.
    ///
    /// The tenant is part of the question: a subject of another
    /// account answers with the same empty a clean subject does, and
    /// the query is not even sent.
    pub async fn rows_reaching(
        &self,
        tenant: &Hrn,
        subject: &Hrn,
    ) -> Result<Vec<ImpactRow>, String> {
        if subject.account_id() != tenant.account_id() {
            return Ok(Vec::new());
        }
        let subject_text = subject.to_string();
        let query = format!(
            "{{ q(func: eq(hodei.impact.route_node, {})) @filter(eq(hodei.impact.tenant, {})) \
             {{ hodei.impact.advisory hodei.impact.subjects hodei.impact.updated_at }} }}",
            dql_string(&subject_text),
            dql_string(&tenant.to_string()),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| error.to_string())?;
        let candidates = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut rows = Vec::new();
        for candidate in candidates {
            let advisory = candidate
                .get("hodei.impact.advisory")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "a route node has no advisory".to_owned())?;
            let stored: Vec<&str> = match candidate.get("hodei.impact.subjects") {
                Some(serde_json::Value::Array(values)) => values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect(),
                Some(serde_json::Value::String(value)) => vec![value.as_str()],
                _ => Vec::new(),
            };
            let depth = stored.iter().find_map(|row| {
                let (name, depth) = row.rsplit_once('|')?;
                (name == subject_text)
                    .then(|| depth.parse::<usize>().ok())
                    .flatten()
            });
            let Some(depth) = depth else { continue };
            let updated_at = candidate
                .get("hodei.impact.updated_at")
                .and_then(serde_json::Value::as_str)
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|instant| instant.with_timezone(&chrono::Utc))
                .unwrap_or_else(chrono::Utc::now);
            rows.push(ImpactRow {
                tenant: tenant.to_string(),
                advisory: advisory.to_owned(),
                subject: subject_text.clone(),
                kind: if depth == 0 { "component" } else { "artifact" }.to_owned(),
                depth,
                updated_at,
            });
        }
        rows.sort_by(|left, right| left.advisory.cmp(&right.advisory));
        Ok(rows)
    }

    /// Whether the view of a tenant is up to date with the deltas the
    /// frontier has said: the MINIMUM watermark of its routes against
    /// the ledger's version. False means there is LAG — and a tenant
    /// that never materialized anything is not current either.
    pub async fn is_current(&self, tenant: &Hrn) -> Result<bool, String> {
        let said = self.said_version(tenant).await?;
        let query = format!(
            "{{ q(func: eq(hodei.impact.tenant, {})) {{ hodei.impact.watermark }} }}",
            dql_string(&tenant.to_string()),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| error.to_string())?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if rows.is_empty() {
            return Ok(false);
        }
        // El MINIMO, no el maximo: un advisory al dia no enmascara el
        // lag de otro. La vista de la cuenta esta al dia cuando TODAS
        // sus claves alcanzan la version del registro.
        let watermark = rows
            .iter()
            .filter_map(|row| {
                row.get("hodei.impact.watermark")
                    .and_then(serde_json::Value::as_u64)
            })
            .min()
            .unwrap_or(0);
        Ok(watermark >= said)
    }

    /// The answer for one advisory, with the freshness said, never
    /// disguised. A current view answers from the view; a stale or
    /// absent one answers from the bounded walk NOW, and the lag stays
    /// said. The account of the advisory and the one asking have to be
    /// the SAME: a mismatch is a closed door, not an empty answer.
    ///
    /// Fail-closed: a store that fails answers with
    /// [`ImpactReadError::Unavailable`], never with an answer that
    /// looks like zero impact, and a walk that hit a bound refuses
    /// instead of answering partially.
    pub async fn serve_impact(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactAnswer, ImpactReadError> {
        if advisory.account_id() != tenant.account_id() {
            return Err(ImpactReadError::TenantMismatch {
                served: tenant.account_id().to_string(),
                requested: advisory.account_id().to_string(),
            });
        }

        let said = self
            .said_version(tenant)
            .await
            .map_err(ImpactReadError::Unavailable)?;
        let stored = self
            .stored_route(tenant, &advisory.to_string())
            .await
            .map_err(ImpactReadError::Unavailable)?;
        let view_watermark = stored.as_ref().map(|route| route.route.watermark);

        if let Some(watermark) = view_watermark.filter(|watermark| *watermark >= said) {
            // The view is complete up to what the frontier has said:
            // answer the view, with its provenance.
            let route = &stored
                .as_ref()
                .expect("the watermark was just read from it")
                .route;
            let mut subjects = route.paths.clone();
            subjects.sort_by(|left, right| left.subject.cmp(&right.subject));
            return Ok(ImpactAnswer {
                tenant: tenant.to_string(),
                advisory: advisory.to_string(),
                subjects,
                deployments: route.deployments.clone(),
                version: watermark,
                freshness: Freshness::Current,
            });
        }

        // The fallback: the bounded walk answers NOW, and the lag it
        // substituted stays said. A walk that hit a bound is refused:
        // a partial answer is never dressed as the authoritative one.
        let walk = walk_impact_dgraph(self.graph.as_ref(), tenant, advisory, &self.limits)
            .await
            .map_err(|error| ImpactReadError::Unavailable(error.to_string()))?;
        if walk.trace.incomplete {
            return Err(ImpactReadError::Unavailable(format!(
                "the impact walk hit its bounds ({:?}): the answer would be partial, and partial is not an answer",
                walk.trace.reasons
            )));
        }
        Ok(ImpactAnswer {
            tenant: tenant.to_string(),
            advisory: advisory.to_string(),
            subjects: walk.answer_subjects(),
            deployments: walk.deployments.iter().cloned().collect(),
            version: said,
            freshness: Freshness::Authoritative {
                view_watermark: view_watermark.unwrap_or(0),
            },
        })
    }

    /// The watermark of the risk consumer, as the frontier stores it:
    /// the next delta version the consumer has to process. Explicit
    /// state in the engine — a restart reads it back instead of
    /// guessing.
    pub async fn consumer_watermark(
        &self,
        consumer: &str,
        stream_id: &str,
    ) -> Result<Option<u64>, String> {
        self.frontier
            .checkpoint_position(consumer, stream_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// Advances the consumer's watermark through the frontier: the
    /// delivery commits as ONE fact (the operation marker and the
    /// checkpoint — a watermark advance says no changeset and
    /// therefore no delta). The same delivery re-driven answers with
    /// its committed receipt without writing again.
    pub async fn advance_consumer_watermark(
        &self,
        consumer: &str,
        stream_id: &str,
        position: u64,
        event_id: &str,
    ) -> Result<ProjectionReceipt, String> {
        let delivery = ProjectionDelivery {
            consumer: consumer.to_owned(),
            stream_id: stream_id.to_owned(),
            position,
            event_id: event_id.to_owned(),
        };
        self.frontier
            .apply_and_checkpoint(GraphChangeSet::empty(), &delivery)
            .await
            .map_err(|error| error.to_string())
    }
}

/// One route node, as it is stored: the route the rebuild wrote and the
/// subject rows it carried.
#[derive(Debug, Clone)]
struct StoredRoute {
    route: ImpactRoute,
    /// The subjects with their depths, decoded from the stored rows.
    subjects: Vec<(String, usize)>,
}

/// The N-Quads of one route node, written from the blank label the
/// transaction unifies: `route` for the update branch and `fresh` for
/// the create branch, same content.
fn route_nquads(route: &ImpactRoute, subjects: &[(String, usize)], label: &str) -> String {
    let mut nquads = String::new();
    let node =
        |predicate: &str, value: &str| format!("_:{label} <{predicate}> {} .\n", dql_string(value));
    nquads.push_str(&format!(
        "_:{label} <dgraph.type> \"{IMPACT_ROUTE_TYPE}\" .\n",
    ));
    nquads.push_str(&node(
        "hodei.impact.key",
        &DgraphImpactView::route_key(&route.tenant, &route.advisory),
    ));
    nquads.push_str(&node("hodei.impact.tenant", &route.tenant));
    nquads.push_str(&node("hodei.impact.advisory", &route.advisory));
    for relation in &route.relations {
        nquads.push_str(&node("hodei.impact.route_relation", relation));
    }
    for evidence in &route.evidences {
        nquads.push_str(&node("hodei.impact.route_evidence", evidence));
    }
    for endpoint in &route.nodes {
        nquads.push_str(&node("hodei.impact.route_node", endpoint));
    }
    for (subject, depth) in subjects {
        nquads.push_str(&node(
            "hodei.impact.subjects",
            &format!("{subject}|{depth}"),
        ));
    }
    if let Ok(paths) = serde_json::to_string(&route.paths) {
        nquads.push_str(&node("hodei.impact.paths", &paths));
    }
    for deployment in &route.deployments {
        nquads.push_str(&node("hodei.impact.deployments", deployment));
    }
    nquads.push_str(&format!(
        "_:{label} <hodei.impact.watermark> \"{}\" .\n",
        route.watermark,
    ));
    nquads.push_str(&node(
        "hodei.impact.updated_at",
        &route.updated_at.to_rfc3339(),
    ));
    nquads
}

/// One stored route node, read back. A node that cannot be read whole
/// is an error, reported — never a default that would hide a hole.
fn stored_route_of(
    row: &serde_json::Value,
    tenant: &Hrn,
    advisory: &str,
) -> Result<StoredRoute, String> {
    let strings = |field: &str| -> Vec<String> {
        match row.get(field) {
            Some(serde_json::Value::Array(rows)) => rows
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            Some(serde_json::Value::String(text)) => vec![text.clone()],
            _ => Vec::new(),
        }
    };
    let watermark = row
        .get("hodei.impact.watermark")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "the route node has no watermark".to_owned())?;
    let paths: Vec<SubjectPath> = row
        .get("hodei.impact.paths")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default();
    let updated_at = row
        .get("hodei.impact.updated_at")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|instant| instant.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let mut subjects: Vec<(String, usize)> = strings("hodei.impact.subjects")
        .into_iter()
        .filter_map(|row| {
            let (subject, depth) = row.rsplit_once('|')?;
            Some((subject.to_owned(), depth.parse::<usize>().ok()?))
        })
        .collect();
    subjects.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(StoredRoute {
        route: ImpactRoute {
            tenant: tenant.to_string(),
            advisory: advisory.to_owned(),
            relations: strings("hodei.impact.route_relation"),
            evidences: strings("hodei.impact.route_evidence"),
            nodes: strings("hodei.impact.route_node"),
            paths,
            deployments: strings("hodei.impact.deployments"),
            watermark,
            updated_at,
        },
        subjects,
    })
}

/// The read-only port, over the native view.
#[async_trait::async_trait]
impl ImpactReader for DgraphImpactView {
    async fn serve_impact(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactAnswer, ImpactReadError> {
        DgraphImpactView::serve_impact(self, tenant, advisory).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("an HRN")
    }

    fn advisory() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-1").expect("an HRN")
    }

    /// The route key is the tenant and the advisory, nothing else: the
    /// same pair is the same route, another account is another route.
    #[test]
    fn the_route_key_scopes_the_advisory() {
        let key = DgraphImpactView::route_key(&tenant().to_string(), &advisory().to_string());
        assert!(key.starts_with("hrn:hodei:artifacts:eu-west-1:acme"));
        assert!(key.contains("GHSA-1"));
        let foreign = DgraphImpactView::route_key(
            "hrn:hodei:artifacts:eu-west-1:mallory:Artifact/mallory",
            &advisory().to_string(),
        );
        assert_ne!(key, foreign, "two tenants never share a route");
    }

    /// The subjects decode from their stored rows: the depth is after
    /// the last separator, the subject keeps everything before it.
    #[test]
    fn the_subject_rows_decode_subject_and_depth() {
        let row = format!("{}|1", tenant());
        let (subject, depth) = row.rsplit_once('|').expect("the row splits");
        assert_eq!(subject, tenant().to_string());
        assert_eq!(depth.parse::<usize>(), Ok(1usize));
    }

    /// A walk that answered whole is not incomplete, and the depth of
    /// an artifact is the level of its component plus one.
    #[test]
    fn the_walk_renders_depths_the_reference_way() {
        let mut walk = DgraphImpactWalk::default();
        let component = "hrn:hodei:artifacts:eu-west-1:acme:Component/lib";
        let artifact = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app";
        walk.affected_components.insert(component.to_owned());
        walk.artifacts.insert(artifact.to_owned(), usize::MAX);
        // No container claimed the artifact: the authoritative answer
        // says one, and the walk says the same.
        let subjects = walk.snapshot_subjects();
        assert_eq!(
            subjects,
            vec![
                (artifact.to_owned(), 1usize),
                (component.to_owned(), 0usize)
            ]
        );
    }

    /// The default bounds of the view are the probe bounds: a
    /// small-to-medium graph answers whole, and tighter bounds are the
    /// operator's explicit decision.
    #[test]
    fn the_default_limits_are_the_probe_bounds() {
        let limits = GraphReadLimits::probe();
        assert!(limits.max_nodes >= 100_000);
        assert!(limits.max_edges >= 400_000);
        assert!(limits.max_round_trips >= 1_000);
    }
}
