//! The store of the risk: the relations of the risk graph and the
//! questions that are asked of them.
//!
//! The relations live in the graph of trust, because that is what the
//! graph is for: an artefact that holds a component, a component that
//! depends on another, and a component that a vulnerability is about. The
//! adapter here reads them and gives them back in the shape the traversal
//! wants.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use hodei_evidence::domain::graph::{GraphChangeSet, RelationType};
use hodei_evidence::ports::graph::{GraphQuery, GraphWriter};
use hodei_risk::ports::RiskRelationError;
use hodei_risk::traversal::Edge;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The reader of the risk relations of an account.
pub struct RiskRelationReader<C: Connection> {
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
}

impl<C: Connection> RiskRelationReader<C> {
    /// A reader over a graph.
    pub fn new(graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>) -> Self {
        Self { graph }
    }

    /// The edges of an account, in the shape the traversal wants.
    ///
    /// The edges are read for the kinds that the risk uses: an artefact
    /// holds a component, a component depends on another, a component is
    /// affected by a vulnerability, and an artefact is deployed as
    /// something. The rest of the graph is not asked for, because asking
    /// for it would be paying for something nobody wanted.
    pub async fn edges(&self, tenant: &Hrn) -> Result<Vec<Edge>, RiskError> {
        let mut edges = Vec::new();

        for kind in [
            hodei_evidence::domain::graph::NodeKind::Artifact,
            hodei_evidence::domain::graph::NodeKind::Component,
        ] {
            let nodes = self
                .graph
                .nodes(tenant, kind)
                .await
                .map_err(|error| RiskError::Unavailable(error.to_string()))?;
            for node in nodes {
                let relations = self
                    .graph
                    .relations_from(tenant, &node.id)
                    .await
                    .map_err(|error| RiskError::Unavailable(error.to_string()))?;
                for relation in relations {
                    if !is_risk_relation(relation.relation.clone()) {
                        continue;
                    }
                    edges.push(Edge {
                        from: relation.from,
                        to: relation.to,
                        relation: relation.relation.clone(),
                    });
                }
            }
        }

        Ok(edges)
    }
}

/// Whether the relation is one of the risk relations.
///
/// The deployments are among them: a blast radius that does not say where
/// the affected artefact runs is a blast radius that does not answer the
/// question.
fn is_risk_relation(relation: RelationType) -> bool {
    matches!(
        relation,
        RelationType::Contains
            | RelationType::DependsOn
            | RelationType::AffectedBy
            | RelationType::DeployedAs
    )
}

/// Why the risk relations could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "the risk store is not available: {message}")
            }
        }
    }
}

impl std::error::Error for RiskError {}

/// Writes a change set of the risk into the graph.
pub async fn apply<C: Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    changes: GraphChangeSet,
) -> Result<(), RiskError> {
    graph
        .apply(changes)
        .await
        .map_err(|error| RiskError::Unavailable(error.to_string()))
}

/// The reader of the risk relations, over a database.
pub fn reader_over<C: Connection>(db: Arc<Surreal<C>>) -> Arc<RiskRelationReader<C>> {
    Arc::new(RiskRelationReader::new(Arc::new(
        hodei_evidence_storage::SurrealGraphStore::new(db),
    )))
}

#[async_trait]
impl<C: Connection> hodei_risk::ports::RiskRelationSource for RiskRelationReader<C> {
    async fn edges(&self, tenant: &Hrn) -> Result<Vec<Edge>, RiskRelationError> {
        RiskRelationReader::edges(self, tenant)
            .await
            .map_err(|error| RiskRelationError::Unavailable(error.to_string()))
    }
}
