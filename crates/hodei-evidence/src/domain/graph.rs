//! The graph of evidence.
//!
//! The graph is a **projection** of the ledger of events, not a second
//! truth. The ledger is the history; the graph is what can be rebuilt from
//! it. That is why nothing writes a relation directly: the projector reads
//! the accepted events and says what they mean, and the writer applies what
//! the projector said.
//!
//! # The nodes of M1
//!
//! Only the ones the first vertical slice needs:
//!
//! ```text
//! Repository -[HAS_REVISION]-> SourceRevision
//! SourceRevision -[TRIGGERED]-> Build
//! Build -[USED_SOURCE]-> SourceRevision
//! Build -[PRODUCED]-> Artifact
//! Artifact -[DEPLOYED_AS]-> Deployment
//! Build -[DESCRIBED_BY]-> Evidence
//! ```
//!
//! # What a relation carries
//!
//! A relation is not a link: it says **which evidence supports it** and
//! **when it was observed**. A relation without evidence is a claim, and a
//! claim is not part of a graph that answers questions about trust.

use crate::domain::evidence::EvidenceId;
use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The kind of a node of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A repository of source.
    Repository,
    /// A revision of a repository.
    SourceRevision,
    /// A build that produced something.
    Build,
    /// An artefact, identified by its digest.
    Artifact,
    /// A deployment of an artefact.
    Deployment,
    /// A piece of accepted evidence.
    Evidence,
}

impl NodeKind {
    /// The name of the kind, as it is written.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Repository => "repository",
            Self::SourceRevision => "source_revision",
            Self::Build => "build",
            Self::Artifact => "artifact",
            Self::Deployment => "deployment",
            Self::Evidence => "evidence",
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The type of a relation of the graph.
///
/// The types are named after what they say. They are not anonymous links,
/// because the question the graph answers --"what produced this artefact?"--
/// depends on the type of the relation and not only on its ends.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// A repository has a revision.
    HasRevision,
    /// A revision triggered a build.
    Triggered,
    /// A build used a revision as its source.
    UsedSource,
    /// A build produced an artefact.
    Produced,
    /// An artefact was deployed as something.
    DeployedAs,
    /// A build is described by a piece of evidence.
    DescribedBy,
}

impl RelationType {
    /// The name of the type, as it is written.
    pub fn as_str(&self) -> &str {
        match self {
            Self::HasRevision => "has_revision",
            Self::Triggered => "triggered",
            Self::UsedSource => "used_source",
            Self::Produced => "produced",
            Self::DeployedAs => "deployed_as",
            Self::DescribedBy => "described_by",
        }
    }
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A node of the graph.
///
/// The identity of a node is its HRN: the same identity the rest of Hodei
/// uses, so a node can be pointed at without inventing a second vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// The identity of the node.
    pub id: Hrn,
    /// The account the node belongs to.
    pub tenant_hrn: Hrn,
    /// The kind of the node.
    pub kind: NodeKind,
    /// The facts about the node, as they were observed.
    #[serde(default)]
    pub attributes: serde_json::Map<String, serde_json::Value>,
    /// When the graph learned about it.
    pub observed_at: DateTime<Utc>,
}

impl GraphNode {
    /// A node with nothing else known about it.
    pub fn new(id: Hrn, tenant_hrn: Hrn, kind: NodeKind, observed_at: DateTime<Utc>) -> Self {
        Self {
            id,
            tenant_hrn,
            kind,
            attributes: serde_json::Map::new(),
            observed_at,
        }
    }

    /// The node, with a fact about it.
    pub fn with_attribute(mut self, name: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(name.into(), value);
        self
    }
}

/// A relation of the graph.
///
/// A relation says what it relates, and **which evidence supports it**. A
/// relation without evidence is a claim; the graph keeps the evidence so
/// that the claim can be checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRelation {
    /// The type of the relation.
    pub relation: RelationType,
    /// The node the relation starts at.
    pub from: Hrn,
    /// The node the relation ends at.
    pub to: Hrn,
    /// The account the relation belongs to.
    pub tenant_hrn: Hrn,
    /// The evidence that supports the relation.
    pub evidence_id: EvidenceId,
    /// When the relation was observed.
    pub observed_at: DateTime<Utc>,
}

impl GraphRelation {
    /// A relation supported by a piece of evidence.
    pub fn new(
        relation: RelationType,
        from: Hrn,
        to: Hrn,
        tenant_hrn: Hrn,
        evidence_id: EvidenceId,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            relation,
            from,
            to,
            tenant_hrn,
            evidence_id,
            observed_at,
        }
    }

    /// The identity of the relation.
    ///
    /// It is the relation itself --its type and its two ends-- so that the
    /// same relation between the same nodes, supported by different
    /// evidence, is the same relation with more support and not two
    /// relations.
    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.relation.as_str(), self.from, self.to)
    }
}

/// The claim that an envelope makes about the graph.
///
/// The evidence says things about the world: this build produced this
/// artefact. That is what the graph is made of, and the claim is what
/// carries it from the envelope to the event and from the event to the
/// graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationClaim {
    /// The type of the relation that is claimed.
    pub relation: RelationType,
    /// The node the relation starts at.
    pub from: Hrn,
    /// The node the relation ends at.
    pub to: Hrn,
    /// The kind of the node the relation starts at.
    pub from_kind: NodeKind,
    /// The kind of the node the relation ends at.
    pub to_kind: NodeKind,
}

/// The changes that the projector says the graph needs.
///
/// A change set is the only way the graph changes. It is written at once or
/// not at all, so the graph never holds half of a change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphChangeSet {
    /// The nodes that have to be in the graph.
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    /// The relations that have to be in the graph.
    #[serde(default)]
    pub relations: Vec<GraphRelation>,
}

impl GraphChangeSet {
    /// An empty change set.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the change set would change nothing.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.relations.is_empty()
    }

    /// The change set, with a node.
    pub fn with_node(mut self, node: GraphNode) -> Self {
        self.nodes.push(node);
        self
    }

    /// The change set, with a relation.
    pub fn with_relation(mut self, relation: GraphRelation) -> Self {
        self.relations.push(relation);
        self
    }

    /// The number of nodes and relations in the change set.
    pub fn len(&self) -> usize {
        self.nodes.len() + self.relations.len()
    }

    /// Whether the change set is empty.
    pub fn is_unchanged(&self) -> bool {
        self.is_empty()
    }
}
