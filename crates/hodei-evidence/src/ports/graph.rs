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
