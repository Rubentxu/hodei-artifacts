//! The graph of evidence, over the store that already exists.
//!
//! The graph is a projection: the ledger is the history and the graph is
//! what can be rebuilt from it. This adapter writes what the projector says
//! and answers questions about lineage.
//!
//! # Why the projection has a name
//!
//! A rebuild builds a projection **apart** from the one that is in use, and
//! compares the two before the new one is promoted. That is why the tables
//! carry the name of their projection: the active graph and the one being
//! rebuilt live side by side, and neither of them is truncated while the
//! other is being checked.
//!
//! # Why there is no index
//!
//! The same reason as the ledger: defining an index on the local engine
//! keeps the database directory locked by the process that defines it, and a
//! service that declared an index on its own path could not restart. The
//! queries are scoped by the account and by the kind, and they lean on what
//! the tables already hold.

use hodei_evidence::GraphChangeSet;
use hodei_evidence::domain::graph::{GraphNode, GraphRelation, NodeKind};
use hodei_evidence::ports::graph::{GraphError, GraphQuery, GraphWriter, Lineage};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The name of the projection that is in use.
pub const ACTIVE_PROJECTION: &str = "graph";

/// The name of the projection that a rebuild builds into.
pub const REBUILD_PROJECTION: &str = "graph_rebuild";

/// The row of a node, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRow {
    /// The identity of the node.
    pub node_id: String,
    /// The account the node belongs to.
    pub tenant_hrn: String,
    /// The kind of the node.
    pub kind: String,
    /// The facts about the node.
    #[serde(default)]
    pub attributes: serde_json::Value,
    /// When the graph learned about it.
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

/// The row of a relation, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRow {
    /// The identity of the relation.
    pub relation_id: String,
    /// The account the relation belongs to.
    pub tenant_hrn: String,
    /// The type of the relation.
    pub relation_type: String,
    /// The node the relation starts at.
    pub from: String,
    /// The node the relation ends at.
    pub to: String,
    /// The evidence that supports the relation.
    pub evidence_id: String,
    /// When the relation was observed.
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

/// The graph, over the store that already exists.
pub struct SurrealGraphStore<C: Connection> {
    db: Arc<Surreal<C>>,
    projection: String,
}

impl<C: Connection> SurrealGraphStore<C> {
    /// The graph that is in use.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self {
            db,
            projection: ACTIVE_PROJECTION.to_string(),
        }
    }

    /// A graph with a name of its own.
    ///
    /// The rebuild uses it to build a projection apart from the one in use.
    pub fn named(db: Arc<Surreal<C>>, projection: impl Into<String>) -> Self {
        Self {
            db,
            projection: projection.into(),
        }
    }

    /// The name of the projection.
    pub fn projection(&self) -> &str {
        &self.projection
    }

    /// The table of the nodes.
    fn node_table(&self) -> String {
        format!("{}_node", self.projection)
    }

    /// The table of the relations.
    fn relation_table(&self) -> String {
        format!("{}_relation", self.projection)
    }

    /// The name of a table, checked before it is written into a statement.
    ///
    /// The statements that work on a whole table need its name written out:
    /// the store does not take a table as a bound value. The name of a
    /// projection comes from the code, but it is checked anyway, because a
    /// name that is written into a statement has no business carrying
    /// anything but letters, digits and underscores.
    fn checked_table(name: &str) -> Result<String, GraphError> {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_');
        if !valid {
            return Err(GraphError::Rejected(format!(
                "the name of a table is not a plain name: {name}"
            )));
        }
        Ok(name.to_string())
    }

    /// The identifier of the record of a node.
    ///
    /// The table is the one of the projection, and not a fixed one: the
    /// rebuild writes into a projection of its own, and its records have to
    /// live in its tables.
    fn node_id(&self, hrn: &Hrn) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((self.node_table(), escape_id(hrn.to_string().as_str())))
    }

    /// The identifier of the record of a relation.
    fn relation_id(&self, identity: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((self.relation_table(), escape_id(identity)))
    }
}

/// Escapes a part of a record identifier.
fn escape_id(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// The row of a node.
fn node_row(node: &GraphNode) -> NodeRow {
    NodeRow {
        node_id: node.id.to_string(),
        tenant_hrn: node.tenant_hrn.to_string(),
        kind: node.kind.as_str().to_string(),
        attributes: serde_json::to_value(&node.attributes).unwrap_or(serde_json::Value::Null),
        observed_at: node.observed_at,
    }
}

/// The row of a relation.
fn relation_row(relation: &GraphRelation) -> RelationRow {
    RelationRow {
        relation_id: relation.identity(),
        tenant_hrn: relation.tenant_hrn.to_string(),
        relation_type: relation.relation.as_str().to_string(),
        from: relation.from.to_string(),
        to: relation.to.to_string(),
        evidence_id: relation.evidence_id.as_str().to_string(),
        observed_at: relation.observed_at,
    }
}

/// The node of a row.
fn node_of(row: &NodeRow) -> Option<GraphNode> {
    let id = Hrn::from_string(&row.node_id)?;
    let tenant = Hrn::from_string(&row.tenant_hrn)?;
    let kind = match row.kind.as_str() {
        "repository" => NodeKind::Repository,
        "source_revision" => NodeKind::SourceRevision,
        "build" => NodeKind::Build,
        "artifact" => NodeKind::Artifact,
        "deployment" => NodeKind::Deployment,
        "evidence" => NodeKind::Evidence,
        _ => return None,
    };
    let attributes = match row.attributes.as_object() {
        Some(attributes) => attributes.clone(),
        None => serde_json::Map::new(),
    };
    Some(GraphNode {
        id,
        tenant_hrn: tenant,
        kind,
        attributes,
        observed_at: row.observed_at,
    })
}

/// The relation of a row.
fn relation_of(row: &RelationRow) -> Option<GraphRelation> {
    use hodei_evidence::domain::graph::RelationType;
    let from = Hrn::from_string(&row.from)?;
    let to = Hrn::from_string(&row.to)?;
    let tenant = Hrn::from_string(&row.tenant_hrn)?;
    let relation = match row.relation_type.as_str() {
        "has_revision" => RelationType::HasRevision,
        "triggered" => RelationType::Triggered,
        "used_source" => RelationType::UsedSource,
        "produced" => RelationType::Produced,
        "deployed_as" => RelationType::DeployedAs,
        "described_by" => RelationType::DescribedBy,
        _ => return None,
    };
    Some(GraphRelation {
        relation,
        from,
        to,
        tenant_hrn: tenant,
        evidence_id: hodei_evidence::EvidenceId::new(row.evidence_id.clone()),
        observed_at: row.observed_at,
    })
}

#[async_trait::async_trait]
impl<C: Connection> GraphWriter for SurrealGraphStore<C> {
    /// Applies a change set.
    ///
    /// The nodes go first and the relations after, because a relation that
    /// points at a node that is not there yet would be a relation that
    /// nobody can follow.
    async fn apply(&self, changes: GraphChangeSet) -> Result<(), GraphError> {
        for node in &changes.nodes {
            self.db
                .query("UPSERT $id CONTENT $content")
                .bind(("id", self.node_id(&node.id)))
                .bind(("content", node_row(node)))
                .await
                .map_err(|_error| GraphError::Unavailable)?
                .check()
                .map_err(|error| GraphError::Rejected(error.to_string()))?;
        }

        for relation in &changes.relations {
            // A relation without its evidence is a claim, and a claim is not
            // part of a graph that answers questions about trust.
            let evidence =
                hodei_evidence::evidence_hrn(&relation.tenant_hrn, &relation.evidence_id);
            let known = changes.nodes.iter().any(|node| node.id == evidence)
                || self.node_exists(&evidence).await?;
            if !known {
                return Err(GraphError::Rejected(format!(
                    "the relation claims the evidence {} and the graph does not have it",
                    relation.evidence_id.as_str()
                )));
            }

            self.db
                .query("UPSERT $id CONTENT $content")
                .bind(("id", self.relation_id(&relation.identity())))
                .bind(("content", relation_row(relation)))
                .await
                .map_err(|_error| GraphError::Unavailable)?
                .check()
                .map_err(|error| GraphError::Rejected(error.to_string()))?;
        }

        Ok(())
    }

    /// Empties the projection.
    async fn clear(&self) -> Result<(), GraphError> {
        for table in [self.node_table(), self.relation_table()] {
            let table = Self::checked_table(&table)?;
            self.db
                .query(format!("REMOVE TABLE IF EXISTS {table}"))
                .await
                .map_err(|_error| GraphError::Unavailable)?
                .check()
                .map_err(|error| GraphError::Rejected(error.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<C: Connection> GraphQuery for SurrealGraphStore<C> {
    /// The nodes of a kind, of an account.
    async fn nodes(&self, tenant: &Hrn, kind: NodeKind) -> Result<Vec<GraphNode>, GraphError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE tenant_hrn = $tenant AND kind = $kind",
            )
            .bind(("table", self.node_table()))
            .bind(("tenant", tenant.to_string()))
            .bind(("kind", kind.as_str().to_string()))
            .await
            .map_err(|_error| GraphError::Unavailable)?;
        let rows: Vec<NodeRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(rows.iter().filter_map(node_of).collect())
    }

    /// The relations that start at a node, of an account.
    async fn relations_from(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        self.relations_where(tenant, "from", &node.to_string())
            .await
    }

    /// The relations that end at a node, of an account.
    async fn relations_to(
        &self,
        tenant: &Hrn,
        node: &Hrn,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        self.relations_where(tenant, "to", &node.to_string()).await
    }

    /// The lineage of an artefact.
    ///
    /// The question the product exists to answer: what produced this
    /// artefact, and what did it come from. The answer walks back from the
    /// artefact to the builds, from the builds to the revisions, and from the
    /// revisions to the repository -- and it says nothing that the graph does
    /// not hold.
    async fn lineage(&self, tenant: &Hrn, artifact: &Hrn) -> Result<Lineage, GraphError> {
        let mut lineage = Lineage::default();

        let artefacts = self.nodes(tenant, NodeKind::Artifact).await?;
        lineage.artifact = artefacts.into_iter().find(|node| &node.id == artifact);

        // The builds that produced it: the relations that END at the
        // artefact are the ones that say who produced it.
        let produced = self.relations_to(tenant, artifact).await?;
        let evidence = produced.clone();
        for relation in &produced {
            if relation.relation == hodei_evidence::RelationType::Produced {
                lineage.builds.extend(
                    self.node_of_kind(tenant, &relation.from, NodeKind::Build)
                        .await?,
                );
            }
        }

        // The revisions those builds used, and the repository they belong to.
        // The revision can be named by two relations --the one that says the
        // build used it, and the one that says it triggered the build-- and
        // both say the same thing, so it is only counted once.
        for build in &lineage.builds {
            for relation in self.relations_to(tenant, &build.id).await? {
                let is_source = relation.relation == hodei_evidence::RelationType::UsedSource;
                let is_trigger = relation.relation == hodei_evidence::RelationType::Triggered;
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
            let has = self.relations_to(tenant, &revision.id).await?;
            for relation in &has {
                if relation.relation == hodei_evidence::RelationType::HasRevision {
                    let repositories = self
                        .node_of_kind(tenant, &relation.from, NodeKind::Repository)
                        .await?;
                    lineage.repository = repositories.into_iter().next();
                }
            }
        }

        // The deployments of the artefact: the relations that START at it.
        let deployed = self.relations_from(tenant, artifact).await?;
        for relation in &deployed {
            if relation.relation == hodei_evidence::RelationType::DeployedAs {
                lineage.deployments.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Deployment)
                        .await?,
                );
            }
        }

        lineage.evidence = evidence;
        Ok(lineage)
    }
}

impl<C: Connection> SurrealGraphStore<C> {
    /// The relations where a field matches a value, of an account.
    async fn relations_where(
        &self,
        tenant: &Hrn,
        field: &str,
        value: &str,
    ) -> Result<Vec<GraphRelation>, GraphError> {
        let query = format!(
            "SELECT * FROM type::table($table) WHERE tenant_hrn = $tenant AND {field} = $value"
        );
        let mut response = self
            .db
            .query(query)
            .bind(("table", self.relation_table()))
            .bind(("tenant", tenant.to_string()))
            .bind(("value", value.to_string()))
            .await
            .map_err(|_error| GraphError::Unavailable)?;
        let rows: Vec<RelationRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(rows.iter().filter_map(relation_of).collect())
    }

    /// Whether the graph has a node with this identity.
    async fn node_exists(&self, id: &Hrn) -> Result<bool, GraphError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", self.node_id(id)))
            .await
            .map_err(|_error| GraphError::Unavailable)?;
        let rows: Vec<NodeRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(!rows.is_empty())
    }

    /// A node of a kind, by its identity.
    async fn node_of_kind(
        &self,
        tenant: &Hrn,
        id: &Hrn,
        kind: NodeKind,
    ) -> Result<Vec<GraphNode>, GraphError> {
        Ok(self
            .nodes(tenant, kind)
            .await?
            .into_iter()
            .filter(|node| &node.id == id)
            .collect())
    }
}
