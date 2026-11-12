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
use hodei_risk::case::BlastRadius;
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

/// The blast radius of a vulnerability, walked in the graph.
///
/// The walk is **bounded**: it starts at the vulnerability and it only
/// visits the nodes that can be reached from it. Reading every relation
/// of the account and filtering afterwards would be O(N) in the size of
/// the account, and a question whose answer is a handful of nodes does
/// not have a right to cost the whole account.
///
/// Every step of the walk is a read by the end of a relation, and the
/// indexes of the graph are exactly on that.
pub async fn blast_radius<C: Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    tenant: &kernel::Hrn,
    vulnerability: &kernel::Hrn,
) -> Result<BlastRadius, RiskError> {
    use hodei_evidence::domain::graph::RelationType;
    use std::collections::{BTreeMap, BTreeSet};

    // The components the vulnerability is about.
    let affected = relations_to(graph, tenant, vulnerability, RelationType::AffectedBy).await?;

    // The walk goes up: the dependency is the reverse of the relation.
    // The nodes of a level are asked about TOGETHER, because the cost of
    // a walk of varios saltos is the de los viajes and not the de los
    // datos.
    let mut containers: BTreeMap<String, Vec<kernel::Hrn>> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut level: Vec<kernel::Hrn> = affected.clone();

    while !level.is_empty() {
        let mut next_level: Vec<kernel::Hrn> = Vec::new();

        let containers_of =
            relations_to_many(graph, tenant, &level, RelationType::Contains).await?;
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

        let dependents = relations_to_many(graph, tenant, &level, RelationType::DependsOn).await?;
        for dependent in dependents {
            let key = dependent.to_string();
            if seen.insert(key) {
                next_level.push(dependent);
            }
        }

        // The nodes of the level that were already visited do not go
        // again.
        level = next_level
            .into_iter()
            .filter(|node| seen.insert(node.to_string()))
            .collect();
    }

    // And the deployments of the artefacts that hold them.
    let mut affected_subjects: BTreeSet<String> = BTreeSet::new();
    let mut affected_deployments: BTreeSet<String> = BTreeSet::new();
    let mut affected_components: BTreeSet<String> = BTreeSet::new();

    for component in &affected {
        affected_components.insert(component.to_string());
    }

    let mut artefact_ids: BTreeSet<String> = BTreeSet::new();
    for artefact in containers.values().flatten() {
        artefact_ids.insert(artefact.to_string());
    }
    let artefacts: Vec<kernel::Hrn> = artefact_ids
        .iter()
        .filter_map(|id| kernel::Hrn::from_string(id))
        .collect();

    let deployments =
        relations_from_many(graph, tenant, &artefacts, RelationType::DeployedAs).await?;
    for artefact in &artefacts {
        affected_subjects.insert(artefact.to_string());
    }
    for deployment in deployments {
        affected_deployments.insert(deployment.to_string());
    }

    Ok(BlastRadius {
        affected_subjects: affected_subjects.into_iter().collect(),
        affected_deployments: affected_deployments.into_iter().collect(),
        affected_components: affected_components.into_iter().collect(),
    })
}

/// The relations that start at MANY nodes, of a kind.
pub async fn relations_from_many<C: Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    tenant: &kernel::Hrn,
    nodes: &[kernel::Hrn],
    relation: RelationType,
) -> Result<Vec<kernel::Hrn>, RiskError> {
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<kernel::Hrn> = nodes.to_vec();
    let mut response = graph
        .db()
        .query(
            "SELECT * FROM type::table($table) \
             WHERE tenant_hrn = $tenant AND relation_type = $relation AND from IN $ids",
        )
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .bind(("relation", relation.as_str().to_string()))
        .bind(("ids", ids))
        .await
        .map_err(|error| RiskError::Unavailable(error.to_string()))?;

    let rows: Vec<RelationRow> = response
        .take(0)
        .map_err(|error| RiskError::Unavailable(error.to_string()))?;
    Ok(rows.into_iter().map(|row| row.from).collect())
}

/// The relations that end at MANY nodes, of a kind.
///
/// The walk of varios saltos asks the same question of many nodes, and
/// asking it of one at a time costs one viaje por nodo. The batched
/// reading asks it of all of them at once, and the difference is the
/// difference between a question whose cost is the del grafo and una
/// pregunta cuyo coste es el de los viajes.
///
/// It is here and not in the port because it is a question about how the
/// graph is asked, and the port asks about what the graph says.
pub async fn relations_to_many<C: Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    tenant: &kernel::Hrn,
    nodes: &[kernel::Hrn],
    relation: RelationType,
) -> Result<Vec<kernel::Hrn>, RiskError> {
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<kernel::Hrn> = nodes.to_vec();
    let mut response = graph
        .db()
        .query(
            "SELECT * FROM type::table($table) \
             WHERE tenant_hrn = $tenant AND relation_type = $relation AND to IN $ids",
        )
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .bind(("relation", relation.as_str().to_string()))
        .bind(("ids", ids))
        .await
        .map_err(|error| RiskError::Unavailable(error.to_string()))?;

    let rows: Vec<RelationRow> = response
        .take(0)
        .map_err(|error| RiskError::Unavailable(error.to_string()))?;
    Ok(rows.into_iter().map(|row| row.from).collect())
}

/// The relation of a row.
#[derive(Debug, Clone, serde::Deserialize)]
struct RelationRow {
    /// The node the relation starts at.
    from: kernel::Hrn,
    /// The node the relation ends at.
    #[allow(dead_code)]
    to: kernel::Hrn,
}

/// The relations that end at a node, of a kind.
async fn relations_to<C: Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    tenant: &kernel::Hrn,
    node: &kernel::Hrn,
    relation: RelationType,
) -> Result<Vec<kernel::Hrn>, RiskError> {
    use hodei_evidence::ports::graph::GraphQuery;
    Ok(graph
        .relations_to(tenant, node)
        .await
        .map_err(|error| RiskError::Unavailable(error.to_string()))?
        .into_iter()
        .filter(|candidate| candidate.relation == relation)
        .map(|candidate| candidate.from)
        .collect())
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
