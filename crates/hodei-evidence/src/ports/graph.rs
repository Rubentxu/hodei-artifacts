//! The ports of the graph.
//!
//! The graph is written by the projector and read by whoever asks questions
//! about lineage. Both are ports of the domain, and neither mentions a
//! table, an index or a transaction: the graph can be rebuilt into another
//! store, and the domain does not find out.

use crate::domain::event::EventEnvelope;
use crate::domain::graph::{GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType};
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The error of a graph that cannot do its job.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// The graph is not available.
    #[error("the graph is not available: {0}")]
    Unavailable(String),
    /// The change could not be applied.
    #[error("the change could not be applied: {0}")]
    Rejected(String),
}

/// The writer of the graph.
///
/// It applies change sets and nothing else. There is no method to insert a
/// relation: the graph changes because the events said so.
#[async_trait::async_trait]
pub trait GraphWriter: Send + Sync {
    /// Applies a change set.
    ///
    /// It is applied whole or not at all: a graph that holds half of a change
    /// answers questions with half of the truth.
    async fn apply(&self, changes: GraphChangeSet) -> Result<(), GraphError>;

    /// Empties the graph, so that it can be rebuilt from the ledger.
    ///
    /// The rebuild never empties the only active graph before the rebuilt
    /// one has been checked: that is the job of whoever orchestrates the
    /// rebuild, and this port does not make it easy to do wrong.
    async fn clear(&self) -> Result<(), GraphError>;
}

/// The query of the graph.
///
/// Every query is scoped to an account. A graph that can be read across
/// accounts is a graph that leaks.
#[async_trait::async_trait]
pub trait GraphQuery: Send + Sync {
    /// The nodes of a kind, of an account.
    async fn nodes(&self, tenant: &Hrn, kind: NodeKind) -> Result<Vec<GraphNode>, GraphError>;

    /// The relations that start at a node, of an account.
    async fn relations_from(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError>;

    /// The relations that end at a node, of an account.
    async fn relations_to(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError>;

    /// The lineage of an artefact: how it was built and what it came from.
    ///
    /// This is the question the product exists to answer, so it has its own
    /// method and not a general traversal that every caller has to build.
    async fn lineage(&self, tenant: &Hrn, artifact: &Hrn) -> Result<Lineage, GraphError>;

    /// The deployments of an artefact.
    async fn deployments(&self, tenant: &Hrn, artifact: &Hrn)
    -> Result<Vec<GraphNode>, GraphError>;

    /// The artefacts a build produced.
    async fn artifacts_of_build(
        &self,
        tenant: &Hrn,
        build: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError>;

    /// The artefacts of a repository.
    ///
    /// It is the walk of three saltos: el repositorio, sus revisiones, las
    /// construcciones que dispararon y los artefactos que produjeron.
    async fn artifacts_of_repository(
        &self,
        tenant: &Hrn,
        repository: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError>;
}

/// The lineage of an artefact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    /// The artefact itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<GraphNode>,
    /// The builds that produced it.
    #[serde(default)]
    pub builds: Vec<GraphNode>,
    /// The revisions those builds used.
    #[serde(default)]
    pub revisions: Vec<GraphNode>,
    /// The repository the revisions belong to.
    #[serde(default)]
    pub repository: Option<GraphNode>,
    /// The deployments of the artefact.
    #[serde(default)]
    pub deployments: Vec<GraphNode>,
    /// The evidence that supports the lineage.
    #[serde(default)]
    pub evidence: Vec<GraphRelation>,
}

/// The error of a projector that could not project an event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    /// The event is not of a shape the projector understands.
    #[error("the event is not of a shape the projector understands: {0}")]
    Shape(String),
    /// The projector cannot work right now.
    #[error("the projector cannot work right now: {0}")]
    Unavailable(String),
}

/// The projector of the graph.
///
/// It reads the events that were accepted and says what the graph has to
/// look like. It is a pure function of the events: the same events produce
/// the same change set, whatever order they are read in and however many
/// times they are read.
///
/// It can fail, and it has to: a projection that cannot fail is a
/// projection that cannot be recovered. When it fails, the event stays in
/// the ledger, the checkpoint does not move, and a later run tries again.
pub trait GraphProjector: Send + Sync {
    /// The changes that an event implies.
    fn project(&self, event: &EventEnvelope) -> Result<GraphChangeSet, ProjectionError>;
}

/// The limits of a bounded graph read.
///
/// A read without limits transports whatever the graph holds, and the
/// caller cannot tell the difference between "this is all of it" and
/// "this is what came back before the walk gave up". Every bounded read
/// carries its limits, and a result that hit one of them says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphReadLimits {
    /// How many hops the walk may take from where it starts.
    pub max_depth: u32,
    /// How many node records the answer may carry.
    pub max_nodes: usize,
    /// How many relations the answer may carry.
    pub max_edges: usize,
    /// How many round trips to the store the walk may spend. The
    /// budget is the guard against a walk that answers slowly by
    /// asking many small questions.
    pub max_round_trips: usize,
}

impl Default for GraphReadLimits {
    /// The default bounds: deep enough for the lineage walk (four
    /// hops), wide enough for the lineage of one artefact, and a round
    /// trip budget that a bounded walk does not spend.
    fn default() -> Self {
        Self {
            max_depth: 6,
            max_nodes: 500,
            max_edges: 2_000,
            max_round_trips: 64,
        }
    }
}

impl GraphReadLimits {
    /// The probe bounds of a measurement: wide enough that a
    /// small-to-medium synthetic graph answers whole, so the p95 of a
    /// bounded read against an effectively unbounded one is a
    /// comparison of shapes, not of truncations.
    pub const fn probe() -> Self {
        Self {
            max_depth: 64,
            max_nodes: 100_000,
            max_edges: 400_000,
            max_round_trips: 1_024,
        }
    }
}

/// How a bounded read was resolved: the limits it was given, what it
/// spent and what, if anything, it had to leave behind. It is the
/// receipt of the walk, and it is enough to explain the answer it
/// travels with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalTrace {
    /// The limits the walk was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<GraphReadLimits>,
    /// The hops the walk took.
    #[serde(default)]
    pub hops_taken: u32,
    /// The node records the walk received from the store.
    #[serde(default)]
    pub nodes_seen: usize,
    /// The relations the walk received from the store.
    #[serde(default)]
    pub edges_seen: usize,
    /// The round trips the walk spent.
    #[serde(default)]
    pub round_trips: usize,
    /// Whether the answer is NOT the whole answer. A bounded read is
    /// never best-effort in silence: when a limit was hit, this is
    /// true and the reasons say which one.
    #[serde(default)]
    pub incomplete: bool,
    /// Why the answer is incomplete, in the order the limits were hit:
    /// `depth-limit`, `node-limit`, `edge-limit`, `budget-limit`.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// How many frontier nodes were left unexpanded when the walk
    /// stopped.
    #[serde(default)]
    pub frontier_left: usize,
}

/// The shared bookkeeping of a bounded walk: one place decides when a
/// walk stops and what it says about it, so every adapter accounts the
/// same way and the trace means the same thing wherever it comes from.
#[derive(Debug, Clone)]
pub struct WalkBudget {
    limits: GraphReadLimits,
    depth: u32,
    nodes_seen: usize,
    edges_seen: usize,
    nodes_kept: usize,
    edges_kept: usize,
    round_trips: usize,
    stopped: bool,
    reasons: Vec<String>,
    frontier_left: usize,
}

impl WalkBudget {
    /// The budget of a walk under these limits.
    pub fn new(limits: GraphReadLimits) -> Self {
        Self {
            limits,
            depth: 0,
            nodes_seen: 0,
            edges_seen: 0,
            nodes_kept: 0,
            edges_kept: 0,
            round_trips: 0,
            stopped: false,
            reasons: Vec::new(),
            frontier_left: 0,
        }
    }

    /// Whether the walk may still take a hop and spend a round trip.
    pub fn may_expand(&self) -> bool {
        !self.stopped
            && self.depth < self.limits.max_depth
            && self.round_trips < self.limits.max_round_trips
    }

    /// Records one hop of the walk.
    pub fn hop(&mut self) {
        self.depth += 1;
    }

    /// Records one round trip to the store. Spending the last one of
    /// the budget stops the walk.
    pub fn round_trip(&mut self) {
        self.round_trips += 1;
        if self.round_trips >= self.limits.max_round_trips {
            self.stop("budget-limit");
        }
    }

    /// How many of the `wanted` edges the walk may keep. The edges are
    /// received either way; the cap decides how many travel in the
    /// answer, and a cut stops the walk with `edge-limit`.
    pub fn admit_edges(&mut self, wanted: usize) -> usize {
        self.edges_seen += wanted;
        let remaining = self.limits.max_edges.saturating_sub(self.edges_kept);
        let admitted = wanted.min(remaining);
        self.edges_kept += admitted;
        if wanted > remaining {
            self.stop("edge-limit");
        }
        admitted
    }

    /// How many of the `wanted` nodes the walk may keep. The nodes are
    /// received either way; the cap decides how many travel in the
    /// answer, and a cut stops the walk with `node-limit`.
    pub fn admit_nodes(&mut self, wanted: usize) -> usize {
        self.nodes_seen += wanted;
        let remaining = self.limits.max_nodes.saturating_sub(self.nodes_kept);
        let admitted = wanted.min(remaining);
        self.nodes_kept += admitted;
        if wanted > remaining {
            self.stop("node-limit");
        }
        admitted
    }

    /// Notes that `pending` frontier nodes were left unexpanded. When
    /// the walk had not stopped for another reason, the depth is the
    /// reason; when it had, the reason it stopped with stands and this
    /// only accounts what was left behind.
    pub fn leave_frontier(&mut self, pending: usize) {
        if pending > 0 {
            self.frontier_left += pending;
            if !self.stopped {
                self.stop("depth-limit");
            }
        }
    }

    /// Stops the walk with a reason. Stopping twice adds the second
    /// reason only.
    pub fn stop(&mut self, reason: &str) {
        self.stopped = true;
        if !self.reasons.iter().any(|held| held == reason) {
            self.reasons.push(reason.to_owned());
        }
    }

    /// Whether the walk has stopped.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// The edges the walk may still keep, for the `first` bound of the
    /// next expansion.
    pub fn edges_remaining(&self) -> usize {
        self.limits.max_edges.saturating_sub(self.edges_kept)
    }

    /// The receipt of the walk.
    pub fn trace(&self) -> TraversalTrace {
        TraversalTrace {
            limits: Some(self.limits),
            hops_taken: self.depth,
            nodes_seen: self.nodes_seen,
            edges_seen: self.edges_seen,
            round_trips: self.round_trips,
            incomplete: self.stopped,
            reasons: self.reasons.clone(),
            frontier_left: self.frontier_left,
        }
    }
}

/// The lineage of an artefact, read under limits, with the receipt of
/// the walk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedLineage {
    /// The lineage itself, as the unbounded read would give it.
    pub lineage: Lineage,
    /// How the walk that produced it was resolved.
    pub trace: TraversalTrace,
}

/// The multi-hop traversal of the dependency graph from one node, read
/// under limits, with the receipt of the walk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedTraversal {
    /// The node the walk started at, when the graph holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<GraphNode>,
    /// Every node the walk reached, the root not included. A node that
    /// the walk met twice through a cycle is here once.
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    /// Every relation the walk crossed.
    #[serde(default)]
    pub edges: Vec<GraphRelation>,
    /// How the walk was resolved.
    pub trace: TraversalTrace,
}

/// The bounded queries of the graph.
///
/// The walks run at the store: each hop expands the frontier where the
/// edges live, and only the bounded frontier comes back. The account is
/// a filter on every hop, and the receipt says how the answer was
/// resolved.
#[async_trait::async_trait]
pub trait BoundedGraphQuery: Send + Sync {
    /// The lineage of an artefact under limits.
    async fn bounded_lineage(
        &self,
        tenant: &Hrn,
        artifact: &Hrn,
        limits: &GraphReadLimits,
    ) -> Result<BoundedLineage, GraphError>;

    /// The dependency graph that starts at a node under limits: the
    /// walk follows the asserted relations forward, hop by hop, and
    /// never visits a node twice.
    async fn bounded_dependencies(
        &self,
        tenant: &Hrn,
        node: &Hrn,
        limits: &GraphReadLimits,
    ) -> Result<BoundedTraversal, GraphError>;
}

/// The relations that the projector of M1 knows how to build.
///
/// It is a list and not a registry because M1 has a closed set of them: the
/// ones the first vertical slice needs. When a new relation appears, it is
/// added here on purpose.
pub fn relation_types() -> Vec<RelationType> {
    vec![
        RelationType::HasRevision,
        RelationType::Triggered,
        RelationType::UsedSource,
        RelationType::Produced,
        RelationType::DeployedAs,
        RelationType::DescribedBy,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> GraphReadLimits {
        GraphReadLimits {
            max_depth: 2,
            max_nodes: 4,
            max_edges: 6,
            max_round_trips: 3,
        }
    }

    /// The depth cap stops the walk exactly when the next hop would
    /// exceed it, and the receipt says so.
    #[test]
    fn the_depth_cap_stops_the_walk() {
        let mut budget = WalkBudget::new(limits());
        budget.hop();
        budget.hop();
        assert!(
            !budget.may_expand(),
            "the third hop exceeds the depth of two"
        );
        budget.leave_frontier(2);
        let trace = budget.trace();
        assert_eq!(trace.hops_taken, 2);
        assert!(trace.incomplete);
        assert_eq!(trace.reasons, vec!["depth-limit".to_owned()]);
        assert_eq!(trace.frontier_left, 2);
    }

    /// A complete walk leaves no frontier and says nothing about being
    /// incomplete.
    #[test]
    fn a_complete_walk_is_not_incomplete() {
        let mut budget = WalkBudget::new(limits());
        budget.hop();
        budget.round_trip();
        assert_eq!(budget.admit_edges(3), 3);
        assert_eq!(budget.admit_nodes(2), 2);
        // A walk that spends its depth without leaving anything behind
        // is a complete answer, not an incomplete one: the depth is a
        // declared bound, and nothing was left unexpanded.
        let mut whole = WalkBudget::new(limits());
        for _ in 0..2 {
            whole.hop();
            whole.round_trip();
            whole.admit_edges(3);
            whole.admit_nodes(2);
        }
        assert!(!whole.may_expand(), "the depth of two is spent");
        assert!(
            !whole.stopped(),
            "no limit was hit: the walk just cannot go deeper"
        );
        let trace = whole.trace();
        assert!(!trace.incomplete);
        assert!(trace.reasons.is_empty());
        assert_eq!(trace.hops_taken, 2);
        assert_eq!(trace.round_trips, 2);
    }

    /// The round trip budget stops the walk when it is spent, and the
    /// receipt names it.
    #[test]
    fn the_round_trip_budget_stops_the_walk() {
        let mut budget = WalkBudget::new(limits());
        for _ in 0..3 {
            budget.round_trip();
        }
        assert!(!budget.may_expand(), "the budget of three is spent");
        let trace = budget.trace();
        assert!(trace.incomplete);
        assert_eq!(trace.reasons, vec!["budget-limit".to_owned()]);
        assert_eq!(trace.round_trips, 3);
    }

    /// The edge cap admits what is left and stops the walk once.
    #[test]
    fn the_edge_cap_admits_what_is_left() {
        let mut budget = WalkBudget::new(limits());
        assert_eq!(budget.admit_edges(4), 4);
        assert_eq!(budget.admit_edges(4), 2, "only two of the six are left");
        let trace = budget.trace();
        assert!(trace.incomplete);
        assert_eq!(trace.reasons, vec!["edge-limit".to_owned()]);
        assert_eq!(trace.edges_seen, 8, "what came back is accounted");
        assert_ne!(trace.edges_seen, trace.round_trips);
        assert_eq!(budget.edges_remaining(), 0);
    }

    /// The node cap admits what is left and stops the walk once, and a
    /// second stop adds its reason without repeating the first.
    #[test]
    fn the_node_cap_accumulates_reasons_once() {
        let mut budget = WalkBudget::new(limits());
        assert_eq!(budget.admit_nodes(3), 3);
        assert_eq!(budget.admit_nodes(3), 1, "only one of the four is left");
        budget.leave_frontier(1);
        let trace = budget.trace();
        assert_eq!(
            trace.reasons,
            vec!["node-limit".to_owned()],
            "the node cap is the reason; the frontier it left is accounted without a second reason"
        );
        assert_eq!(trace.frontier_left, 1);
    }

    /// A reason is said once, however many times the walk stopped with
    /// it.
    #[test]
    fn a_reason_is_said_once() {
        let mut budget = WalkBudget::new(limits());
        budget.stop("node-limit");
        budget.stop("node-limit");
        budget.stop("edge-limit");
        let trace = budget.trace();
        assert_eq!(
            trace.reasons,
            vec!["node-limit".to_owned(), "edge-limit".to_owned()]
        );
    }

    /// The probe bounds of a measurement carry the whole of a
    /// small-to-medium graph without stopping.
    #[test]
    fn the_probe_bounds_do_not_stop_a_small_walk() {
        let mut budget = WalkBudget::new(GraphReadLimits::probe());
        for _ in 0..8 {
            budget.hop();
            budget.round_trip();
            budget.admit_edges(1_000);
            budget.admit_nodes(500);
        }
        assert!(!budget.trace().incomplete);
    }
}
