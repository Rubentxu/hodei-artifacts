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
    /// A component of an artefact, named by its coordinates.
    Component,
    /// A vulnerability, named by the identifier of its advisory.
    Vulnerability,
}

impl NodeKind {
    /// The kind of a name, when it is one this version knows.
    ///
    /// The reading lives here and not in the store, so that a kind that is
    /// added to the domain is read everywhere and not only where somebody
    /// remembered to add it. A kind that is not known returns nothing, and
    /// the store does not pretend that an unknown kind is one it knows.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "repository" => Some(Self::Repository),
            "source_revision" => Some(Self::SourceRevision),
            "build" => Some(Self::Build),
            "artifact" => Some(Self::Artifact),
            "deployment" => Some(Self::Deployment),
            "evidence" => Some(Self::Evidence),
            "component" => Some(Self::Component),
            "vulnerability" => Some(Self::Vulnerability),
            _ => None,
        }
    }

    /// The name of the kind, as it is written.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Repository => "repository",
            Self::SourceRevision => "source_revision",
            Self::Build => "build",
            Self::Artifact => "artifact",
            Self::Deployment => "deployment",
            Self::Evidence => "evidence",
            Self::Component => "component",
            Self::Vulnerability => "vulnerability",
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
    /// An artefact contains a component.
    Contains,
    /// A component depends on another component.
    ///
    /// The relation does not say whether the dependency is direct or
    /// transitive: that is a property of the path, and a property of a
    /// path is not a property of an edge. The traversal is what tells
    /// them apart.
    DependsOn,
    /// A component is affected by a vulnerability.
    AffectedBy,
}

impl RelationType {
    /// The type of a name, when it is one this version knows.
    ///
    /// The reading lives here for the same reason that the reading of the
    /// kinds lives here: a type that is added to the domain has to be read
    /// everywhere, and not only en el sitio que alguien recordo.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "has_revision" => Some(Self::HasRevision),
            "triggered" => Some(Self::Triggered),
            "used_source" => Some(Self::UsedSource),
            "produced" => Some(Self::Produced),
            "deployed_as" => Some(Self::DeployedAs),
            "described_by" => Some(Self::DescribedBy),
            "contains" => Some(Self::Contains),
            "depends_on" => Some(Self::DependsOn),
            "affected_by" => Some(Self::AffectedBy),
            _ => None,
        }
    }

    /// The name of the type, as it is written.
    pub fn as_str(&self) -> &str {
        match self {
            Self::HasRevision => "has_revision",
            Self::Triggered => "triggered",
            Self::UsedSource => "used_source",
            Self::Produced => "produced",
            Self::DeployedAs => "deployed_as",
            Self::DescribedBy => "described_by",
            Self::Contains => "contains",
            Self::DependsOn => "depends_on",
            Self::AffectedBy => "affected_by",
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

/// The withdrawal of ONE support of ONE relation.
///
/// A relation is asserted by the evidence that supports it, and one
/// evidence can be withdrawn without touching the others: una evidencia
/// supersedida retira SU apoyo, y los apoyos independientes se quedan.
/// The withdrawal names the relation by its identity --the type and its
/// two ends-- and the evidence whose support goes away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRelationWithdrawal {
    /// The type of the relation whose support is withdrawn.
    pub relation: RelationType,
    /// The node the relation starts at.
    pub from: Hrn,
    /// The node the relation ends at.
    pub to: Hrn,
    /// The account the relation belongs to.
    pub tenant_hrn: Hrn,
    /// The evidence whose support is withdrawn.
    pub evidence_id: EvidenceId,
    /// When the withdrawal was observed.
    pub observed_at: DateTime<Utc>,
}

impl GraphRelationWithdrawal {
    /// A withdrawal of one evidence's support on one relation.
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

    /// The identity of the relation whose support is withdrawn: the same
    /// identity the relation itself carries.
    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.relation.as_str(), self.from, self.to)
    }
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
    /// The supports that have to go away: what a superseded evidence
    /// withdraws, one support at a time, without touching the others.
    #[serde(default)]
    pub withdrawals: Vec<GraphRelationWithdrawal>,
}

impl GraphChangeSet {
    /// An empty change set.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the change set would change nothing.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.relations.is_empty() && self.withdrawals.is_empty()
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

    /// The change set, with a support withdrawal.
    pub fn with_withdrawal(mut self, withdrawal: GraphRelationWithdrawal) -> Self {
        self.withdrawals.push(withdrawal);
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
