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

/// The indexes the graph needs, and the table each one is on.
///
/// The measure of the scale model (SPIKE-012) said the answer: without
/// these, the graph answers a question that touches ten relations by
/// reading every relation there is, and a question of three hops by
/// reading the table three times. With them, the same question went from
/// 435 ms to 4.8 ms on the reference load.
///
/// The declarations are given here, and not in the adapter, because
/// declaring an index on the local engine keeps the database directory
/// locked by the process that declares it. An operator command declares
/// them from its own process and releases the lock when it exits.
pub const GRAPH_INDEXES: &[(&str, &str)] = &[
    ("graph_node", "tenant_hrn, kind"),
    ("graph_relation", "tenant_hrn, from"),
    ("graph_relation", "tenant_hrn, to"),
    ("graph_rebuild_node", "tenant_hrn, kind"),
    ("graph_rebuild_relation", "tenant_hrn, from"),
    ("graph_rebuild_relation", "tenant_hrn, to"),
];

/// The statement that declares an index, and makes it if it is not there.
///
/// The name of the index is derived from the table and from the fields it
/// indexes on, so that the same index is not declared twice under two
/// names.
pub fn declare_index_statement(table: &str, fields: &str) -> String {
    let name = index_name(table, fields);
    format!("DEFINE INDEX IF NOT EXISTS {name} ON TABLE {table} FIELDS {fields}")
}

/// The name of an index, derived from the table and the fields.
///
/// A name is written out in a statement, so it carries nothing but letters,
/// digits and underscores.
fn index_name(table: &str, fields: &str) -> String {
    let mut name = String::with_capacity(table.len() + fields.len() + 1);
    for character in table.chars().chain(fields.chars()) {
        if character.is_ascii_alphanumeric() {
            name.push(character);
        } else {
            name.push('_');
        }
    }
    name
}

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

/// What the database reports about a table.
#[derive(Debug, Default, serde::Deserialize)]
struct TableInfo {
    /// The indexes the table has, by name.
    #[serde(default)]
    indexes: Option<std::collections::BTreeMap<String, String>>,
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

    /// The database the graph is in.
    ///
    /// The operator command that declares the indexes needs the handle, and
    /// the test that proves the indexes are in place needs to declare them
    /// the way the command does.
    pub fn database(&self) -> &Arc<Surreal<C>> {
        &self.db
    }

    /// Whether the indexes the graph needs are in place.
    ///
    /// A missing index costs the graph its speed and not its correctness:
    /// the same questions are answered, and they are answered by reading
    /// more. The answer is reported rather than enforced, so a deployment
    /// that has not run the operator command still serves correctly.
    pub async fn indexes_initialized(&self) -> Result<bool, GraphError> {
        let wanted = GRAPH_INDEXES
            .iter()
            .filter(|(table, _)| *table == self.node_table() || *table == self.relation_table())
            .count();
        let present = self.index_count(&self.node_table()).await?
            + self.index_count(&self.relation_table()).await?;
        Ok(present >= wanted)
    }

    /// How many indexes a table has.
    ///
    /// The name of the table is written out, because the statement does
    /// not take a table as a bound value. The name comes from the code, and
    /// it is checked anyway.
    async fn index_count(&self, table: &str) -> Result<usize, GraphError> {
        let table = Self::checked_table(table)?;
        let mut response = self
            .db
            .query(format!("INFO FOR TABLE {table}"))
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let info: Option<TableInfo> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(info
            .and_then(|info| info.indexes)
            .map(|indexes| indexes.len())
            .unwrap_or_default())
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
            self.upsert(self.node_id(&node.id), node_row(node)).await?;
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

            self.upsert(
                self.relation_id(&relation.identity()),
                relation_row(relation),
            )
            .await?;
        }

        Ok(())
    }

    /// Empties the projection.
    ///
    /// The rows are deleted and the table is kept: removing the table
    /// would remove the indexes with it, and the graph would go back to
    /// reading the whole table for every question. The measure of the
    /// scale model showed what that costs.
    async fn clear(&self) -> Result<(), GraphError> {
        for table in [self.node_table(), self.relation_table()] {
            let table = Self::checked_table(&table)?;
            self.db
                .query(format!("DELETE {table}"))
                .await
                .map_err(|error| GraphError::Unavailable(error.to_string()))?
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
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
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

    /// The deployments of an artefact.
    async fn deployments(
        &self,
        tenant: &Hrn,
        artifact: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut deployments = Vec::new();
        for relation in self.relations_from(tenant, artifact).await? {
            if relation.relation == hodei_evidence::RelationType::DeployedAs {
                deployments.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Deployment)
                        .await?,
                );
            }
        }
        Ok(deployments)
    }

    /// The artefacts a build produced.
    async fn artifacts_of_build(
        &self,
        tenant: &Hrn,
        build: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut artifacts = Vec::new();
        for relation in self.relations_to(tenant, build).await? {
            if relation.relation == hodei_evidence::RelationType::Produced {
                artifacts.extend(
                    self.node_of_kind(tenant, &relation.to, NodeKind::Artifact)
                        .await?,
                );
            }
        }
        Ok(artifacts)
    }

    /// The artefacts of a repository.
    ///
    /// It is the walk of three saltos: el repositorio, sus revisiones, las
    /// construcciones que dispararon y los artefactos que produjeron.
    async fn artifacts_of_repository(
        &self,
        tenant: &Hrn,
        repository: &Hrn,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut artifacts: Vec<GraphNode> = Vec::new();

        for relation in self.relations_from(tenant, repository).await? {
            if relation.relation != hodei_evidence::RelationType::HasRevision {
                continue;
            }
            for trigger in self.relations_from(tenant, &relation.to).await? {
                if trigger.relation != hodei_evidence::RelationType::Triggered {
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

    /// The lineage of an artefact.
    ///
    /// The question the product exists to answer: what produced this
    /// artefact, and what did it come from. The answer walks back from the
    /// artefact to the builds, from the builds to the revisions, and from the
    /// revisions to the repository -- and it says nothing that the graph does
    /// not hold.
    async fn lineage(&self, tenant: &Hrn, artifact: &Hrn) -> Result<Lineage, GraphError> {
        // The artefact is read by its identity: reading all the artefacts of
        // the account to find one of them was the most expensive read of the
        // whole lineage, and the measure of the scale model showed it.
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
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows: Vec<RelationRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(rows.iter().filter_map(relation_of).collect())
    }

    /// Writes a record, and tries again when another writer got there at the
    /// same moment.
    async fn upsert<T: serde::Serialize + Clone + Send + Sync + 'static>(
        &self,
        id: surrealdb::sql::Thing,
        content: T,
    ) -> Result<(), GraphError> {
        let mut attempts = 0;
        loop {
            let written = self
                .db
                .query("UPSERT $id CONTENT $content")
                .bind(("id", id.clone()))
                .bind(("content", content.clone()))
                .await
                .map_err(|error| GraphError::Unavailable(error.to_string()));

            let failure = match written {
                Ok(response) => response.check().err().map(|error| error.to_string()),
                Err(error) => Some(error.to_string()),
            };

            match failure {
                None => return Ok(()),
                Some(message)
                    if crate::is_conflict(&message) && attempts < crate::CONFLICT_RETRIES =>
                {
                    attempts += 1;
                }
                Some(message) => return Err(GraphError::Rejected(message)),
            }
        }
    }

    /// Whether the graph has a node with this identity.
    async fn node_exists(&self, id: &Hrn) -> Result<bool, GraphError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", self.node_id(id)))
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows: Vec<NodeRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(!rows.is_empty())
    }

    /// A node of a kind, by its identity.
    ///
    /// The identity of a node IS the identity of its record, so the node is
    /// read by its identity and not found among the nodes of its kind. The
    /// measure of the scale model found the difference: looking for a node
    /// among all the nodes of a kind reads the whole table, and the lineage
    /// of an artefact does that once per hop.
    ///
    /// The account and the kind are checked on the row that comes back: a
    /// record that belongs to another account, or that is another kind of
    /// node, is not the node that was asked for.
    async fn node_of_kind(
        &self,
        tenant: &Hrn,
        id: &Hrn,
        kind: NodeKind,
    ) -> Result<Vec<GraphNode>, GraphError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", self.node_id(id)))
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows: Vec<NodeRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;

        Ok(rows
            .iter()
            .filter(|row| row.tenant_hrn == tenant.to_string())
            .filter(|row| row.kind == kind.as_str())
            .filter_map(node_of)
            .collect())
    }
}

/// The row of a checkpoint, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRow {
    /// The consumer that projects.
    pub consumer: String,
    /// The stream it projects.
    pub stream_id: String,
    /// The next position it has to project.
    pub next_position: i64,
}

/// The table the checkpoints live in.
///
/// It is a table of its own, and not a table of the graph: emptying the
/// graph to rebuild it does not take the checkpoints with it, because a
/// checkpoint that disappears with the projection would make the rebuild
/// start again from the beginning every time.
pub const CHECKPOINT_TABLE: &str = "projection_checkpoint";

/// The checkpoints of the projections, over the store that already exists.
pub struct SurrealProjectionCheckpoints<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealProjectionCheckpoints<C> {
    /// The checkpoints, over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identifier of the record of a checkpoint.
    fn checkpoint_id(consumer: &str, stream_id: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((
            CHECKPOINT_TABLE.to_string(),
            format!("{}|{}", escape_id(consumer), escape_id(stream_id)),
        ))
    }
}

#[async_trait::async_trait]
impl<C: Connection> hodei_evidence::application::projection::ProjectionCheckpoints
    for SurrealProjectionCheckpoints<C>
{
    /// Where a consumer is.
    async fn checkpoint(&self, consumer: &str, stream_id: &str) -> Result<Option<u64>, GraphError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::checkpoint_id(consumer, stream_id)))
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?;
        let rows: Vec<CheckpointRow> = response
            .take(0)
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(rows.first().map(|row| row.next_position.max(0) as u64))
    }

    /// Says where a consumer is.
    async fn set_checkpoint(
        &self,
        consumer: &str,
        stream_id: &str,
        position: u64,
    ) -> Result<(), GraphError> {
        let row = CheckpointRow {
            consumer: consumer.to_string(),
            stream_id: stream_id.to_string(),
            next_position: position as i64,
        };
        self.db
            .query("UPSERT $id CONTENT $content")
            .bind(("id", Self::checkpoint_id(consumer, stream_id)))
            .bind(("content", row))
            .await
            .map_err(|error| GraphError::Unavailable(error.to_string()))?
            .check()
            .map_err(|error| GraphError::Rejected(error.to_string()))?;
        Ok(())
    }
}
