//! The simulation on the graph authority: the coherent snapshot and the
//! anchored clock (HGS-36, HSP-208).
//!
//! HSP-92 proved the conformance of the simulation over the Surreal
//! reference: the diff is computed, the world is rechecked, and nothing
//! the fork said ever becomes a fact of the graph. This module ports the
//! same observable behaviour to Dgraph, following the precedent of
//! HSP-206 — the read side ported to the graph authority without
//! touching the pure core.
//!
//! # What travels to the graph, and what does not
//!
//! A simulation reads the REAL graph and writes NOTHING to it. The
//! [`SimulationGraph`] port has no write method, and a port without a
//! write method cannot be used to write: the hypothetical overlays of a
//! fork live in memory and in the diff, never in the authority. The only
//! state this module writes is
//!
//! - the fixture of the conformance world — REAL base, said by the
//!   writer port and nothing else;
//! - the anchor row of a simulation: the snapshot watermark and the
//!   fixed clock it was prepared against. The anchor is a record OF the
//!   simulation, in the additive `hodei.simulation.*` namespace of this
//!   adapter; it is not a fact of the world, and the shared schema is
//!   not touched.
//!
//! # The coherent snapshot and the anchored clock
//!
//! A simulation that reads across a moving world returns a diff over a
//! world nobody has. The reads are therefore frozen twice over:
//!
//! - the **snapshot**: the watermark of the tenant's delta story the
//!   fork declared. The port answers its `generation` with the LIVE
//!   watermark — the version the frontier's committed deliveries
//!   reached — so a world that moved is caught by the service's guards
//!   as a stale base, the same answer the Surreal oracle gives; and the
//!   relation reads refuse a world that moved above the sealed snapshot
//!   inside the read itself, which is a coherence guard, not a guess;
//! - the **clock**: the instant the fork evaluates at, frozen when the
//!   anchor was written. Two forks against one anchor give one result,
//!   and a result that depends on the wall clock cannot be audited.
//!
//! The watermark is the same question the freshness checker of HSP-206
//! answers (the delta story the frontier committed), so a support
//! retired after the anchor is seen as a move and never as a hidden
//! fact.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hex::encode as hex_encode;
use hodei_evidence::application::projector::evidence_hrn;
use hodei_evidence::domain::delta::RelationDeltaLedger;
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::{GraphQuery, GraphWriter};
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence::{
    ContentDigest, ContentRef, EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification,
    Normalization, Producer,
};
use hodei_evidence_storage::DgraphGraphStore;
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    dql_string,
};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use hodei_simulation::conformance::{ConformanceWorld, Fixture};
use hodei_simulation::hypothetical::RelationFact;
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};

/// The status the graph answers with: only asserted relations are read.
const STATUS_ASSERTED: &str = "asserted";

const MAX_ALTER_ATTEMPTS: usize = 8;
const ALTER_BACKOFF: Duration = Duration::from_millis(25);
const MAX_WRITE_ATTEMPTS: usize = 3;

/// The Dgraph type of the anchor row. It lives in this adapter's own
/// namespace: the shared schema of `hodei_graph_client::schema` is not
/// touched.
pub const SIMULATION_ANCHOR_TYPE: &str = "HodeiSimulationAnchor";

/// The DDL slice of this adapter: additive predicates and the anchor
/// type, applied idempotently after the versioned schema boot. Re-sending
/// the same text changes nothing on the server.
pub const SIMULATION_SCHEMA_DQL: &str = "
hodei.simulation.anchor.key: string @index(exact) @upsert .
hodei.simulation.anchor.snapshot: int .
hodei.simulation.anchor.clock: datetime .
";

/// The key of the anchor row of one simulation in one account: a
/// deterministic identity, so a restart looks the anchor up instead of
/// finding two.
pub fn anchor_key(simulation: &str, account: &str) -> String {
    hex_encode(
        Sha256::digest(format!("hodei.simulation.anchor#{simulation}#{account}").as_bytes())
            .as_slice(),
    )
}

/// The DQL of one anchor write: an idempotent create-if-absent.
///
/// Two portables from the shared alpha, honoured here:
/// - an upsert's `@if` only sees variables declared as `var as var(...)`:
///   the existence check is a declared var, not an inline `func:`;
/// - every composite key passes through `dql_string`: a raw separator
///   breaks the server's lexer.
pub fn anchor_upsert(key: &str, snapshot: u64, clock: &DateTime<Utc>) -> GraphUpsertTransaction {
    let literal = dql_string(key);
    GraphUpsertTransaction {
        query: format!("{{ exists as var(func: eq(hodei.simulation.anchor.key, {literal})) }}"),
        blocks: vec![GraphMutationBlock::set_if(
            "eq(len(exists), 0)",
            format!(
                "_:a <dgraph.type> \"{SIMULATION_ANCHOR_TYPE}\" .\n\
                 _:a <hodei.simulation.anchor.key> {literal} .\n\
                 _:a <hodei.simulation.anchor.snapshot> {snapshot} .\n\
                 _:a <hodei.simulation.anchor.clock> {} .\n",
                dql_string(&clock.to_rfc3339()),
            ),
        )],
        commit_now: true,
    }
}

/// The DQL that reads one anchor row back.
pub fn anchor_query(key: &str) -> String {
    format!(
        "{{ q(func: eq(hodei.simulation.anchor.key, {})) @filter(type({SIMULATION_ANCHOR_TYPE})) \
         {{ hodei.simulation.anchor.key hodei.simulation.anchor.snapshot \
         hodei.simulation.anchor.clock }} }}",
        dql_string(key),
    )
}

/// Why an anchor could not be written or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnchorError {
    /// The anchor of this simulation and account exists with ANOTHER
    /// snapshot or clock: the world and the instant of a simulation are
    /// frozen once, and re-opening must not silently move the first.
    #[error("the anchor of simulation {simulation} in account {account} is already frozen elsewhere")]
    AlreadyAnchored {
        /// The simulation the key belongs to.
        simulation: String,
        /// The account the key belongs to.
        account: String,
    },
    /// The graph did not answer. Silence is not an anchor: a missing
    /// watermark is never invented with zero.
    #[error("the anchor could not be reached on the graph: {0}")]
    Unavailable(String),
}

/// The frozen pair a simulation evaluates against: the snapshot of the
/// delta story and the instant the clock was fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationAnchor {
    /// The watermark of the tenant's delta story the snapshot is taken
    /// at: the version the history had reached when the simulation was
    /// anchored.
    pub snapshot_generation: u64,
    /// The instant every decision of this simulation evaluates at. Not
    /// the wall clock: a fixed instant is what makes the same base and
    /// the same facts give the same result.
    pub clock: DateTime<Utc>,
}

impl SimulationAnchor {
    /// The anchor at a snapshot and a fixed instant.
    pub fn new(snapshot_generation: u64, clock: DateTime<Utc>) -> Self {
        Self {
            snapshot_generation,
            clock,
        }
    }
}

/// Records the anchor of a simulation in the graph's own namespace.
///
/// It is the only writing piece of the simulation port, and what it
/// writes is metadata of the simulation — never a relation, never
/// evidence, never a fact of the world. The anchor row is created exactly
/// once per (simulation, account): a re-open that replays the very same
/// anchor is fine, a re-open that would move the snapshot or the clock is
/// refused.
pub struct DgraphSimulationAnchor {
    client: Arc<GraphClient>,
}

impl DgraphSimulationAnchor {
    /// The recorder over a connected client.
    pub fn new(client: Arc<GraphClient>) -> Self {
        Self { client }
    }

    /// Applies the additive DDL slice of this adapter. Idempotent: the
    /// same text sent twice changes nothing. Bounded retries: two stores
    /// opening at once alter the same slice, and the server refuses one
    /// of them while the indexes settle.
    pub async fn ensure_schema(client: &GraphClient) -> Result<(), AnchorError> {
        let mut last = String::new();
        for attempt in 1..=MAX_ALTER_ATTEMPTS {
            match GraphMutationPort::alter_schema(client, SIMULATION_SCHEMA_DQL).await {
                Ok(()) => return Ok(()),
                Err(error) if attempt < MAX_ALTER_ATTEMPTS => {
                    last = error.to_string();
                    tokio::time::sleep(ALTER_BACKOFF * attempt as u32).await;
                }
                Err(error) => return Err(AnchorError::Unavailable(error.to_string())),
            }
        }
        Err(AnchorError::Unavailable(format!(
            "the DDL slice did not settle after {MAX_ALTER_ATTEMPTS} attempts: {last}"
        )))
    }

    /// Writes the anchor row if absent, or replays it when it froze the
    /// very same snapshot and clock. The existence check lives inside
    /// the transaction's own query, so the create-if-absent is one
    /// atomic answer.
    pub async fn record(
        &self,
        simulation: &str,
        account: &str,
        anchor: &SimulationAnchor,
    ) -> Result<(), AnchorError> {
        let key = anchor_key(simulation, account);
        for attempt in 0..MAX_WRITE_ATTEMPTS {
            match self
                .client
                .upsert(anchor_upsert(&key, anchor.snapshot_generation, &anchor.clock))
                .await
            {
                Ok(response) => {
                    let rows: Vec<serde_json::Value> = response
                        .json
                        .get("exists")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if rows.is_empty() {
                        return Ok(());
                    }
                    if rows.iter().any(|row| {
                        row.get("hodei.simulation.anchor.snapshot")
                            .and_then(serde_json::Value::as_u64)
                            == Some(anchor.snapshot_generation)
                            && row.get("hodei.simulation.anchor.clock")
                                .and_then(serde_json::Value::as_str)
                                == Some(anchor.clock.to_rfc3339().as_str())
                    }) {
                        // Re-recording the very same anchor is a replay:
                        // the simulation re-declares what it froze.
                        return Ok(());
                    }
                    return Err(AnchorError::AlreadyAnchored {
                        simulation: simulation.to_string(),
                        account: account.to_string(),
                    });
                }
                // Losing the write race on the @upsert key: the other
                // writer committed, and the retry reads what it froze.
                Err(GraphClientError::UpsertConflict { .. })
                    if attempt + 1 < MAX_WRITE_ATTEMPTS => {}                Err(error) => return Err(AnchorError::Unavailable(error.to_string())),
            }
        }
        Err(AnchorError::Unavailable(
            "the anchor write kept conflicting".to_string(),
        ))
    }

    /// Reads the anchor row back: the snapshot and the clock the graph
    /// holds for this simulation and account. `None` when nothing was
    /// anchored — the absence is an answer, not an error.
    pub async fn lookup(
        &self,
        simulation: &str,
        account: &str,
    ) -> Result<Option<SimulationAnchor>, AnchorError> {
        let response = self
            .client
            .execute_dql(&anchor_query(&anchor_key(simulation, account)))
            .await
            .map_err(|error| AnchorError::Unavailable(error.to_string()))?;
        let Some(row) = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.first().cloned())
        else {
            return Ok(None);
        };
        let snapshot = row
            .get("hodei.simulation.anchor.snapshot")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                AnchorError::Unavailable("the anchor row carries no snapshot".to_string())
            })?;
        let clock = row
            .get("hodei.simulation.anchor.clock")
            .and_then(serde_json::Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|instant| instant.with_timezone(&Utc))
            .ok_or_else(|| {
                AnchorError::Unavailable("the anchor row carries no readable clock".to_string())
            })?;
        Ok(Some(SimulationAnchor::new(snapshot, clock)))
    }
}

/// The graph of truth, read for the simulation: the Dgraph port of the
/// HSP-92 adapter.
///
/// The reads are the graph's native traversals, scoped to the tenant,
/// and every relation read is the asserted fact of the account. The port
/// has no write method, and this type has none either: the only path to
/// the graph from here is a query.
pub struct DgraphSimulationGraph {
    client: Arc<GraphClient>,
    graph: Arc<DgraphGraphStore>,
    deltas: Arc<dyn RelationDeltaLedger>,
    tenant: Hrn,
    anchor: SimulationAnchor,
}

impl DgraphSimulationGraph {
    /// The reader of one account, anchored at a snapshot and a clock.
    pub fn new(
        client: Arc<GraphClient>,
        graph: Arc<DgraphGraphStore>,
        deltas: Arc<dyn RelationDeltaLedger>,
        tenant: Hrn,
        anchor: SimulationAnchor,
    ) -> Self {
        Self {
            client,
            graph,
            deltas,
            tenant,
            anchor,
        }
    }

    /// The frozen snapshot and clock this graph evaluates against.
    pub fn the_anchor(&self) -> SimulationAnchor {
        self.anchor
    }

    /// The watermark of the tenant's delta story as the graph holds it
    /// now: the live version of the history the frontier committed. The
    /// service compares this against the generation the fork declares,
    /// and a move says stale and not a diff over a world nobody has.
    pub async fn the_watermark(&self) -> Result<u64, String> {
        self.deltas.version_of(&self.tenant).await
    }

    /// Why the snapshot refused a read: the same text the guards answer
    /// with, said once.
    fn moved_above_the_snapshot(&self, watermark: u64) -> String {
        format!(
            "the world moved above the sealed snapshot: the anchor is generation {} and the story \
             reached {watermark}",
            self.anchor.snapshot_generation
        )
    }

    /// The asserted relations of the tenant, read through the projection's
    /// own tenant-scoped scan. The scan is bounded to the account: on a
    /// shared alpha another tenant's keys are never seen.
    async fn the_relations_of_account(&self) -> Result<Vec<GraphRelation>, String> {
        let tenant_literal = dql_string(&self.tenant.to_string());
        let query = format!(
            "{{ q(func: eq(hodei.graph.rel.tenant, {tenant_literal})) \
             @filter(eq(hodei.graph.rel.status, {status})) {{ \
             hodei.graph.rel.tenant hodei.graph.rel.type hodei.graph.rel.from_hrn \
             hodei.graph.rel.to_hrn hodei.graph.rel.evidence hodei.graph.rel.observed_at }} }}",
            status = dql_string(STATUS_ASSERTED),
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
        Ok(rows.iter().filter_map(relation_of).collect())
    }

    /// A fact of a relation, as the anchored reader answers it.
    fn the_fact(&self, relation: &GraphRelation) -> RelationFact {
        RelationFact::new(
            relation.relation.clone(),
            relation.from.clone(),
            relation.to.clone(),
        )
        .with_evidence(relation.evidence_id.as_str())
    }
}

#[async_trait]
impl SimulationGraph for DgraphSimulationGraph {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        // The base is coherent or the read closes: a relation of the
        // generation ABOVE the sealed snapshot is not part of the world
        // the fork declared, and answering with two generations would
        // return a diff over a world nobody has. Silence is never an
        // empty collection: an error of the store reaches the caller as
        // the error it is.
        let watermark = self.the_watermark().await?;
        if watermark > self.anchor.snapshot_generation {
            return Err(self.moved_above_the_snapshot(watermark));
        }

        let mut facts = Vec::new();
        for relation in GraphQuery::relations_from(&*self.graph, &self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?
        {
            facts.push(self.the_fact(&relation));
        }
        for relation in GraphQuery::relations_to(&*self.graph, &self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?
        {
            let fact = self.the_fact(&relation);
            // The dedup is by IDENTITY: the same relation appears once
            // per end it is touched by.
            if !facts.iter().any(|one| one.is_the_same_as(&fact)) {
                facts.push(fact);
            }
        }
        Ok(facts)
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        let watermark = self.the_watermark().await?;
        if watermark > self.anchor.snapshot_generation {
            return Err(self.moved_above_the_snapshot(watermark));
        }
        let relations = self.the_relations_of_account().await?;
        let mut facts: Vec<RelationFact> = relations.iter().map(|one| self.the_fact(one)).collect();
        facts.sort_by_key(|relation| relation.identity());
        facts.dedup_by_key(|relation| relation.identity());
        Ok(facts)
    }

    async fn relation_count(&self) -> Result<usize, String> {
        Ok(self.all_relations().await?.len())
    }

    async fn generation(&self) -> Result<u64, String> {
        // LIVE: the version the story reached, the same question the
        // Surreal oracle answers. The anchor is what the fork declares
        // against; this is what the world says it is.
        self.the_watermark().await
    }

    fn account(&self) -> String {
        self.tenant.account_id().to_string()
    }
}

/// The conformance world of the graph: the HSP-92 fixture left in the
/// authority, read back through the anchored port.
///
/// The fixture is the REAL base the simulation runs on — said by the
/// graph's writer port with the SAME mechanics the projection uses, and
/// nothing else reaches the graph. The hypothetical overlays and diffs
/// never pass through this type: they live in the forks and in memory,
/// and the graph stays the world as it was.
pub struct DgraphConformance {
    client: Arc<GraphClient>,
    graph: Arc<DgraphGraphStore>,
    deltas: Arc<dyn RelationDeltaLedger>,
    evidence: Arc<dyn EvidenceStore>,
    tenant: Hrn,
    anchor: SimulationAnchor,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl DgraphConformance {
    /// The world over the graph's pieces, anchored at the watermark the
    /// story holds at opening and the fixed conformance instant.
    pub async fn new(
        client: Arc<GraphClient>,
        graph: Arc<DgraphGraphStore>,
        deltas: Arc<dyn RelationDeltaLedger>,
        evidence: Arc<dyn EvidenceStore>,
        tenant: Hrn,
        clock: DateTime<Utc>,
        decisions: Arc<dyn DecisionEvaluator>,
    ) -> Result<Self, AnchorError> {
        DgraphSimulationAnchor::ensure_schema(&client).await?;
        let snapshot_generation = deltas
            .version_of(&tenant)
            .await
            .map_err(AnchorError::Unavailable)?;
        Ok(Self {
            client,
            graph,
            deltas,
            evidence,
            tenant,
            anchor: SimulationAnchor::new(snapshot_generation, clock),
            decisions,
        })
    }

    /// The anchor the scenarios evaluate against.
    pub fn the_anchor(&self) -> SimulationAnchor {
        self.anchor
    }

    /// The anchored reader the service runs on: the graph port a real
    /// consumer would build, with the fixture's snapshot and clock.
    pub fn the_graph(&self) -> Arc<DgraphSimulationGraph> {
        Arc::new(DgraphSimulationGraph::new(
            self.client.clone(),
            self.graph.clone(),
            self.deltas.clone(),
            self.tenant.clone(),
            self.anchor,
        ))
    }
}

#[async_trait]
impl ConformanceWorld for DgraphConformance {
    fn account(&self) -> String {
        self.tenant.account_id().to_string()
    }

    fn tenant(&self) -> Hrn {
        self.tenant.clone()
    }

    async fn prepare(&self, fixture: &Fixture) -> Result<(), String> {
        // The fixture's evidence is part of the world: the graph refuses
        // a relation that claims evidence the authority does not hold,
        // so the evidence records are admitted first — with the
        // conformance clock, never the wall clock.
        for id in [
            hodei_simulation::conformance::THE_FIRST_EVIDENCE,
            hodei_simulation::conformance::THE_SECOND_EVIDENCE,
        ] {
            let record = EvidenceRecord::builder(EvidenceId::new(id), fixture.tenant.clone())
                .subjects(vec![fixture.artefact.clone()])
                .evidence_type(EvidenceType::Sbom)
                .producer(Producer::new("connector", "hsp208").with_version("1.0.0"))
                .content(ContentRef::by_digest(ContentDigest::of_bytes(
                    format!("hsp208-conformance-{id}").as_bytes(),
                )))
                .observed_at(self.anchor.clock)
                .recorded_at(self.anchor.clock)
                .verification(EvidenceVerification::unverified())
                .normalization(Normalization::new("raw", "1.0.0"))
                .build();
            self.evidence
                .put(record)
                .await
                .map_err(|error| error.to_string())?;
        }

        let now = self.anchor.clock;
        let mut changes = GraphChangeSet::empty();

        // The nodes of the fixture.
        for node in &fixture.nodes {
            changes = changes.with_node(GraphNode::new(
                node.hrn.clone(),
                fixture.tenant.clone(),
                node.kind.clone(),
                now,
            ));
        }

        // And the nodes of the evidence: a relation that says it is held
        // up by evidence that does not exist is not written.
        for relation in &fixture.relations {
            let Some(evidence) = relation.evidence.as_ref() else {
                continue;
            };
            changes = changes.with_node(GraphNode::new(
                evidence_hrn(&fixture.tenant, &EvidenceId::new(evidence.clone())),
                fixture.tenant.clone(),
                NodeKind::Evidence,
                now,
            ));
        }

        // And the relations.
        for relation in &fixture.relations {
            let evidence = EvidenceId::new(relation.evidence.clone().unwrap_or_default());
            changes = changes.with_relation(GraphRelation::new(
                relation.relation.clone(),
                relation.from.clone(),
                relation.to.clone(),
                fixture.tenant.clone(),
                evidence,
                now,
            ));
        }

        GraphWriter::apply(&*self.graph, changes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn relations(&self) -> Result<Vec<RelationFact>, String> {
        self.the_graph().all_relations().await
    }

    async fn generation(&self) -> Result<u64, String> {
        // Live, exactly as the Surreal oracle answers: the version the
        // story of the tenant reached.
        self.deltas.version_of(&self.tenant).await
    }

    fn decisions(&self) -> Arc<dyn DecisionEvaluator> {
        self.decisions.clone()
    }
}

/// The row of a relation, read from the graph's own answer. Mirrors the
/// projection's parser so a row the graph answers with is a fact the
/// simulation can compare, and anything unparseable is absent — never a
/// half-relation that would seal a manifest on noise.
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
    Some(GraphRelation {
        relation,
        from,
        to,
        tenant_hrn: tenant,
        evidence_id: EvidenceId::new(evidence),
        observed_at: row
            .get("hodei.graph.rel.observed_at")
            .and_then(serde_json::Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|instant| instant.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
    })
}
