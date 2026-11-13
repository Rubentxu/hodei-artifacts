//! The view of the impact, materialized inside SurrealDB.
//!
//! The authoritative answer of "what does this vulnerability reach" is
//! the walk of the graph: it is always right, and it costs a traversal
//! every time. This view is the **projection** of that answer: the
//! subjects, their depths and the routes of support that reach them,
//! said ONCE and kept up to date with the deltas of the relations --
//! lo que cambio se invalida SOLO donde cambio, y lo que esta al dia se
//! lee sin recorrer nada.
//!
//! # The watermark
//!
//! The view says UP TO WHICH version of the deltas it is complete. A
//! reader that needs the truth of the moment compares that version with
//! the ledger's, and the lag is EXPOSED, never hidden: un resultado
//! viejo presentado como actual es una falsa seguridad.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use hodei_evidence::domain::delta::{RelationDelta, RelationDeltaLedger};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence_storage::surreal::delta::SurrealRelationDeltaLedger;
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use surrealdb::Connection;

/// The table the materialized view lives in.
pub const IMPACT_TABLE: &str = "impact_view";

/// The table the watermarks of the view live in.
pub const IMPACT_META_TABLE: &str = "impact_meta";

/// A row of the view: ONE subject of ONE advisory, with its depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactRow {
    /// The account the view belongs to.
    pub tenant: String,
    /// The advisory the view is about.
    pub advisory: String,
    /// The subject the advisory reaches.
    pub subject: String,
    /// What the subject is: artifact | component.
    pub kind: String,
    /// How far the dependency is: the depth of the walk.
    pub depth: usize,
    /// When the row was written.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// How fresh an answer is. La frescura se EXPLICA, nunca se disfraza:
/// una vista al dia es Current, y una respuesta calculada ahora por el
/// recorrido autoritativo dice cual era el watermark atrasado que
/// sustituyo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Freshness {
    /// The view was complete up to the version of the ledger.
    Current,
    /// The answer was computed NOW by the authoritative walk: la vista
    /// estaba atrasada o no estaba.
    Authoritative { view_watermark: u64 },
}

/// One subject of the answer, with the path that explains it: las
/// relaciones y la evidencia que lo trajeron. El que pregunta puede
/// comprobar el camino.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectPath {
    /// The subject, as an HRN string.
    pub subject: String,
    /// How far the dependency is: zero for the component the
    /// vulnerability is in.
    pub depth: usize,
    /// The identities of the relations that brought this subject in.
    pub relations: Vec<String>,
    /// The evidence that supports each of those relations.
    pub evidences: Vec<String>,
}

/// The answer of the impact, typed and agent-readable. Es de SOLO
/// LECTURA por construccion: no hay metodo que mutar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactAnswer {
    /// The account the answer belongs to.
    pub tenant: String,
    /// The advisory asked about.
    pub advisory: String,
    /// The subjects, with the path that explains each one.
    pub subjects: Vec<SubjectPath>,
    /// The deployments that run the affected artifacts.
    pub deployments: Vec<String>,
    /// The version the answer is complete up to.
    pub version: u64,
    /// Where the answer came from, and how fresh it is.
    pub freshness: Freshness,
}

/// Why serving an answer failed. Un desajuste de cuenta NO es una
/// respuesta vacia: es una puerta cerrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpactReadError {
    /// The advisory belongs to another account than the one served.
    TenantMismatch { served: String, requested: String },
    /// The advisory is not a canonical HRN.
    NotCanonical(String),
    /// The store failed underneath.
    Unavailable(String),
}

impl std::fmt::Display for ImpactReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TenantMismatch { served, requested } => write!(
                formatter,
                "the advisory belongs to account {requested} and this reader serves {served}"
            ),
            Self::NotCanonical(value) => write!(formatter, "{value} is not a canonical HRN"),
            Self::Unavailable(reason) => write!(formatter, "the store is unavailable: {reason}"),
        }
    }
}

impl std::error::Error for ImpactReadError {}

/// The READ-ONLY port of the impact. El consumidor pregunta, y la
/// respuesta dice de donde sale y hasta que version es completa; el
/// consumidor NO puede mutar nada por esta puerta.
#[async_trait::async_trait]
pub trait ImpactReader: Send + Sync {
    /// The answer for one advisory of one account.
    async fn serve_impact(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactAnswer, ImpactReadError>;
}

/// The route of one advisory: the relations its walk used, with the
/// evidence that supports each. Es la procedencia de la vista: el que
/// pregunta puede comprobar el camino, y el que invalida sabe que
/// cambios tocan este advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImpactRoute {
    /// The account the route belongs to.
    pub tenant: String,
    /// The advisory the route belongs to.
    pub advisory: String,
    /// The identities of the relations the walk used.
    pub relations: Vec<String>,
    /// The evidences that support those relations.
    pub evidences: Vec<String>,
    /// The nodes the walk traversed. Un edge NUEVO no esta en ninguna
    /// relacion previa: lo que invalida un edge nuevo es que sus
    /// extremos pasan por la ruta.
    #[serde(default)]
    pub nodes: Vec<String>,
    /// The path that explains each subject, as it was walked.
    #[serde(default)]
    pub paths: Vec<SubjectPath>,
    /// The deployments that run the affected artifacts.
    #[serde(default)]
    pub deployments: Vec<String>,
    /// The version of the deltas the route was computed at.
    pub watermark: u64,
    /// When the route was written.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// What the view answers for one advisory, with the version it is
/// complete up to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactSnapshot {
    /// The advisory.
    pub advisory: String,
    /// The subjects the advisory reaches, with their depths.
    pub subjects: Vec<(String, usize)>,
    /// The version the view is complete up to.
    pub watermark: u64,
}

/// The materialized view, over the graph and the ledger of deltas that
/// already exist. No hay almacenes nuevos: la vista vive en el mismo
/// SurrealDB, con los indices que el operador administra.
pub struct SurrealImpactView<C: Connection> {
    graph: Arc<SurrealGraphStore<C>>,
    deltas: Arc<SurrealRelationDeltaLedger<C>>,
}

impl<C: Connection> SurrealImpactView<C> {
    /// The view over the graph and the ledger of deltas.
    pub fn new(
        graph: Arc<SurrealGraphStore<C>>,
        deltas: Arc<SurrealRelationDeltaLedger<C>>,
    ) -> Self {
        Self { graph, deltas }
    }

    /// The raw rows of the view, for the reader that counts them.
    pub async fn the_rows(&self) -> Result<Vec<serde_json::Value>, String> {
        let mut response = self
            .graph
            .db()
            .query("SELECT * FROM type::table($table)")
            .bind(("table", IMPACT_TABLE))
            .await
            .map_err(|error| error.to_string())?;
        response
            .take(0)
            .map_err(|error| error.to_string())
    }

    /// The record id of one route.
    fn route_id(tenant: &str, advisory: &str) -> surrealdb::sql::Thing {
        let key = escape_id(&format!("{tenant}|{advisory}"));
        surrealdb::sql::Thing::from((IMPACT_META_TABLE.to_string(), key))
    }

    /// The record id of one subject row.
    fn row_id(tenant: &str, advisory: &str, subject: &str) -> surrealdb::sql::Thing {
        let key = escape_id(&format!("{tenant}|{advisory}|{subject}"));
        surrealdb::sql::Thing::from((IMPACT_TABLE.to_string(), key))
    }

    /// The watermark of one advisory's view, when it has one.
    async fn watermark_of(&self, tenant: &Hrn, advisory: &str) -> Result<Option<u64>, String> {
        let rows: Vec<ImpactRoute> = self
            .graph
            .db()
            .query("SELECT * FROM $id")
            .bind(("id", Self::route_id(&tenant.to_string(), advisory)))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        Ok(rows.first().map(|route| route.watermark))
    }

    /// Materializes the impact of ONE advisory, from the authoritative
    /// walk, at the version the ledger has reached. It is IDEMPOTENT:
    /// recomputar lo mismo deja lo mismo.
    pub async fn rebuild_advisory(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactSnapshot, String> {
        let walk = walk_impact(self.graph.as_ref(), tenant, advisory).await?;
        let watermark = self.deltas.version_of(tenant).await?;
        let now = chrono::Utc::now();

        // The route: the relations the walk used, the evidence that
        // supports each, and the path that explains each subject.
        let mut paths: Vec<SubjectPath> = Vec::new();
        for (subject, edges) in &walk.subject_routes {
            let is_row = walk.artifacts.contains_key(subject)
                || walk.affected_components.contains(subject);
            if !is_row {
                continue;
            }
            let depth = walk
                .artifacts
                .get(subject)
                .map(|depth| if *depth == usize::MAX { 1 } else { *depth })
                .unwrap_or(0);
            paths.push(SubjectPath {
                subject: subject.clone(),
                depth,
                relations: edges.iter().map(|(relation, _)| relation.clone()).collect(),
                evidences: edges.iter().map(|(_, evidence)| evidence.clone()).collect(),
            });
        }
        paths.sort_by(|left, right| left.subject.cmp(&right.subject));
        let route = ImpactRoute {
            tenant: tenant.to_string(),
            advisory: advisory.to_string(),
            relations: walk.route_relations.into_iter().collect(),
            evidences: walk.route_evidence.into_iter().collect(),
            nodes: walk.route_nodes.into_iter().collect(),
            paths,
            deployments: walk.deployments.iter().cloned().collect(),
            watermark,
            updated_at: now,
        };

        // The subjects of the advisory, with their depths. The rows go
        // in ONE transaction with the route: la vista de un advisory se
        // escribe entera o no se escribe.
        let mut subjects: Vec<(String, usize)> = Vec::new();
        let mut sql = String::from("BEGIN TRANSACTION;\n");
        // The rebuild REPLACES the view of the advisory: lo que el nuevo
        // recorrido ya no alcanza se borra, no se hereda.
        sql.push_str(
            "DELETE FROM type::table($table) WHERE tenant = $tenant AND advisory = $advisory;\n",
        );
        sql.push_str("UPSERT $rid CONTENT $route;\n");
        let mut index = 0;
        // The depth of an artifact that no component claimed: the
        // authoritative answer says one, y la vista dice lo mismo.
        for (subject, depth) in walk.artifacts.iter() {
            let depth = if *depth == usize::MAX { 1 } else { *depth };
            sql.push_str(&format!("UPSERT $rowid{index} CONTENT $row{index};\n"));
            subjects.push((subject.clone(), depth));
            index += 1;
        }
        for subject in &walk.affected_components {
            sql.push_str(&format!("UPSERT $rowid{index} CONTENT $row{index};\n"));
            subjects.push((subject.clone(), 0));
            index += 1;
        }
        sql.push_str("COMMIT;\n");

        let db = self.graph.db();
        let mut statement = db.query(sql);
        statement = statement
            .bind(("table", IMPACT_TABLE))
            .bind(("tenant", tenant.to_string()))
            .bind(("advisory", advisory.to_string()))
            .bind(("rid", Self::route_id(&tenant.to_string(), &advisory.to_string())))
            .bind(("route", route));
        let mut index = 0;
        for (subject, depth) in walk.artifacts.iter() {
            let depth = if *depth == usize::MAX { 1 } else { *depth };
            statement = statement
                .bind((
                    format!("rowid{index}"),
                    Self::row_id(&tenant.to_string(), &advisory.to_string(), subject),
                ))
                .bind((
                    format!("row{index}"),
                    ImpactRow {
                        tenant: tenant.to_string(),
                        advisory: advisory.to_string(),
                        subject: subject.clone(),
                        kind: "artifact".to_string(),
                        depth,
                        updated_at: now,
                    },
                ));
            index += 1;
        }
        for subject in &walk.affected_components {
            statement = statement
                .bind((
                    format!("rowid{index}"),
                    Self::row_id(&tenant.to_string(), &advisory.to_string(), subject),
                ))
                .bind((
                    format!("row{index}"),
                    ImpactRow {
                        tenant: tenant.to_string(),
                        advisory: advisory.to_string(),
                        subject: subject.clone(),
                        kind: "component".to_string(),
                        depth: 0,
                        updated_at: now,
                    },
                ));
            index += 1;
        }
        statement
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;

        subjects.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(ImpactSnapshot {
            advisory: advisory.to_string(),
            subjects,
            watermark,
        })
    }

    /// What a delta invalidates: the advisories whose routes used the
    /// relation the delta talks about, or the evidence it moved. SOLO
    /// las claves afectadas: lo demas de la vista no se toca.
    pub async fn invalidated_by(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String> {
        let mut response = self
            .graph
            .db()
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE tenant = $tenant \
                 AND (relations CONTAINS $relation \
                      OR evidences CONTAINS $evidence \
                      OR nodes CONTAINS $from \
                      OR nodes CONTAINS $to)",
            )
            .bind(("table", IMPACT_META_TABLE))
            .bind(("tenant", delta.tenant.to_string()))
            .bind(("relation", delta.identity()))
            .bind(("evidence", delta.evidence.as_str().to_string()))
            .bind(("from", delta.from.to_string()))
            .bind(("to", delta.to.to_string()))
            .await
            .map_err(|error| error.to_string())?;
        let routes: Vec<ImpactRoute> = response
            .take(0)
            .map_err(|error| error.to_string())?;
        let mut advisories: Vec<Hrn> = routes
            .iter()
            .filter_map(|route| Hrn::from_string(&route.advisory))
            .collect();
        advisories.sort_by_key(|advisory| advisory.to_string());
        advisories.dedup();
        Ok(advisories)
    }

    /// Applies a delta to the view: recomputes ONLY the advisories the
    /// delta touches, and answers with them. Una reentrega del mismo
    /// delta es el mismo trabajo una vez, porque el registro deduplica
    /// por causa.
    pub async fn apply_delta(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String> {
        let advisories = self.invalidated_by(delta).await?;
        for advisory in &advisories {
            self.rebuild_advisory(&delta.tenant, advisory).await?;
        }
        Ok(advisories)
    }

    /// The answer for one advisory, with the freshness said, never
    /// disguised. Una vista al dia responde de la vista; una vista
    /// atrasada o ausente responde del recorrido autoritativo AHORA, y
    /// el lag queda expuesto. La cuenta del advisory y la del que
    /// pregunta tienen que ser la MISMA: un desajuste es una puerta
    /// cerrada, no una respuesta vacia.
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
            .deltas
            .version_of(tenant)
            .await
            .map_err(ImpactReadError::Unavailable)?;
        let view_watermark = self
            .watermark_of(tenant, &advisory.to_string())
            .await
            .map_err(ImpactReadError::Unavailable)?;

        if let Some(watermark) = view_watermark.filter(|watermark| *watermark >= said) {
            // The view is complete up to what the ledger has said:
            // responde la vista, con su procedencia.
            let mut response = self
                .graph
                .db()
                .query("SELECT * FROM type::table($table) WHERE tenant = $tenant AND advisory = $advisory")
                .bind(("table", IMPACT_TABLE))
                .bind(("tenant", tenant.to_string()))
                .bind(("advisory", advisory.to_string()))
                .await
                .map_err(|error| ImpactReadError::Unavailable(error.to_string()))?;
            let rows: Vec<ImpactRow> = response
                .take(0)
                .map_err(|error| ImpactReadError::Unavailable(error.to_string()))?;
            let routes: Vec<ImpactRoute> = self
                .graph
                .db()
                .query("SELECT * FROM $id")
                .bind(("id", Self::route_id(&tenant.to_string(), &advisory.to_string())))
                .await
                .map_err(|error| ImpactReadError::Unavailable(error.to_string()))?
                .take(0)
                .map_err(|error| ImpactReadError::Unavailable(error.to_string()))?;
            let route = routes.first();
            let mut subjects: Vec<SubjectPath> = rows
                .into_iter()
                .map(|row| {
                    let path = route.and_then(|route| {
                        route.paths.iter().find(|path| path.subject == row.subject)
                    });
                    SubjectPath {
                        subject: row.subject,
                        depth: row.depth,
                        relations: path.map(|path| path.relations.clone()).unwrap_or_default(),
                        evidences: path.map(|path| path.evidences.clone()).unwrap_or_default(),
                    }
                })
                .collect();
            subjects.sort_by(|left, right| left.subject.cmp(&right.subject));
            return Ok(ImpactAnswer {
                tenant: tenant.to_string(),
                advisory: advisory.to_string(),
                subjects,
                deployments: route.map(|route| route.deployments.clone()).unwrap_or_default(),
                version: watermark,
                freshness: Freshness::Current,
            });
        }

        // The fallback: the authoritative walk answers NOW, and the lag
        // it substituted stays said.
        let walk = walk_impact(self.graph.as_ref(), tenant, advisory)
            .await
            .map_err(ImpactReadError::Unavailable)?;
        Ok(ImpactAnswer {
            tenant: tenant.to_string(),
            advisory: advisory.to_string(),
            subjects: the_answer_subjects(&walk),
            deployments: walk.deployments.iter().cloned().collect(),
            version: said,
            freshness: Freshness::Authoritative {
                view_watermark: view_watermark.unwrap_or(0),
            },
        })
    }

    /// The view of one advisory, with the version it is complete up to.
    /// The reader that needs the truth of the moment compares the
    /// watermark with the ledger's version: el lag se expone, nunca se
    /// esconde.
    pub async fn read(&self, tenant: &Hrn, advisory: &Hrn) -> Result<Option<ImpactSnapshot>, String> {
        let Some(watermark) = self.watermark_of(tenant, &advisory.to_string()).await? else {
            return Ok(None);
        };
        let mut response = self
            .graph
            .db()
            .query("SELECT * FROM type::table($table) WHERE tenant = $tenant AND advisory = $advisory")
            .bind(("table", IMPACT_TABLE))
            .bind(("tenant", tenant.to_string()))
            .bind(("advisory", advisory.to_string()))
            .await
            .map_err(|error| error.to_string())?;
        let rows: Vec<ImpactRow> = response
            .take(0)
            .map_err(|error| error.to_string())?;
        let mut subjects: Vec<(String, usize)> = rows
            .into_iter()
            .map(|row| (row.subject, row.depth))
            .collect();
        subjects.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Some(ImpactSnapshot {
            advisory: advisory.to_string(),
            subjects,
            watermark,
        }))
    }

    /// Whether the view of a tenant is up to date with the deltas that
    /// have been said: la version de la vista contra la version del
    /// registro. False means there is LAG, and a reader that presents
    /// the view as current would be presenting stale results.
    pub async fn is_current(&self, tenant: &Hrn) -> Result<bool, String> {
        let said = self.deltas.version_of(tenant).await?;
        let rows: Vec<ImpactRoute> = self
            .graph
            .db()
            .query("SELECT * FROM type::table($table) WHERE tenant = $tenant")
            .bind(("table", IMPACT_META_TABLE))
            .bind(("tenant", tenant.to_string()))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        // Sin vista no hay actualidad: una cuenta que nunca materializo
        // nada no se presenta como al dia.
        if rows.is_empty() {
            return Ok(false);
        }
        // El MINIMO, no el maximo: un advisory al dia no enmascara el
        // lag de otro. La vista de la cuenta esta al dia cuando TODAS
        // sus claves alcanzan la version del registro.
        let watermark = rows.iter().map(|route| route.watermark).min().unwrap_or(0);
        Ok(watermark >= said)
    }
}

/// Escapes a part of a record identifier, with the same rules the rest
/// of the stores use.
fn escape_id(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// What one walk of the impact saw: los sujetos que el advisory
/// alcanza, con sus profundidades, y las rutas de soporte que usó.
#[derive(Debug, Default)]
pub struct ImpactWalk {
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
    /// The nodes the walk traversed: los extremos de todo lo que uso.
    pub route_nodes: BTreeSet<String>,
    /// The edges that discovered each subject: (relation identity,
    /// evidence id), the path that explains it.
    pub subject_routes: BTreeMap<String, Vec<(String, String)>>,
}

/// The walk records what it used as it uses it: the identity of the
/// relation, the evidence that supports it and the nodes it joins, for
/// the route of the advisory and for the ones that invalidate.
fn record_route(
    route_relations: &mut BTreeSet<String>,
    route_evidence: &mut BTreeSet<String>,
    route_nodes: &mut BTreeSet<String>,
    anchors: &[String],
    rows: &[SupportStepRow],
) {
    // Los extremos de cada paso entran en la ruta: lo que el walk uso
    // es tambien por donde puede tocarle un cambio.
    for anchor in anchors {
        route_nodes.insert(anchor.clone());
    }
    for row in rows {
        route_relations.insert(row.relation_id.clone());
        route_evidence.insert(row.evidence_id.clone());
        route_nodes.insert(row.from.clone());
    }
}

/// A step of the walk, with the identity and the evidence of the
/// relation that made it.
#[derive(Debug, Clone, Deserialize)]
pub struct SupportStepRow {
    /// The identity of the relation.
    pub relation_id: String,
    /// The evidence that supports the relation.
    pub evidence_id: String,
    /// The node the relation starts at.
    pub from: String,
}

/// The walk of the impact, shared by the authoritative answer and by
/// the view: the same traversal, the same cost, and the route recorded
/// as it goes.
pub async fn walk_impact<C: Connection>(
    graph: &SurrealGraphStore<C>,
    tenant: &Hrn,
    vulnerability: &Hrn,
) -> Result<ImpactWalk, String> {
    // The components the vulnerability is about, WITH their route step.
    let affected_rows = relations_to_rows(graph, tenant, vulnerability, RelationType::AffectedBy).await?;
    let mut walk = ImpactWalk::default();
    let mut affected: Vec<Hrn> = Vec::new();
    for row in &affected_rows {
        if let Some(hrn) = Hrn::from_string(&row.from) {
            affected.push(hrn);
        }
        walk.subject_routes
            .entry(row.from.clone())
            .or_default()
            .push((row.relation_id.clone(), row.evidence_id.clone()));
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
        let mut next_level: Vec<Hrn> = Vec::new();

        let asked: Vec<String> = level.iter().map(|node| node.to_string()).collect();
        let container_rows =
            relations_to_many_rows(graph, tenant, &level, RelationType::Contains).await?;
        record_route(
            &mut walk.route_relations,
            &mut walk.route_evidence,
            &mut walk.route_nodes,
            &asked,
            &container_rows,
        );
        for row in &container_rows {
            walk.subject_routes
                .entry(row.from.clone())
                .or_default()
                .push((row.relation_id.clone(), row.evidence_id.clone()));
        }
        let containers_of: Vec<Hrn> = container_rows
            .iter()
            .filter_map(|row| Hrn::from_string(&row.from))
            .collect();
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

        let dependent_rows =
            relations_to_many_rows(graph, tenant, &level, RelationType::DependsOn).await?;
        record_route(
            &mut walk.route_relations,
            &mut walk.route_evidence,
            &mut walk.route_nodes,
            &asked,
            &dependent_rows,
        );
        let dependents: Vec<Hrn> = dependent_rows
            .iter()
            .filter_map(|row| Hrn::from_string(&row.from))
            .collect();
        for dependent in dependents {
            let key = dependent.to_string();
            if seen.insert(key.clone()) {
                levels.insert(key, depth + 1);
                next_level.push(dependent);
            }
        }

        level = next_level;
        depth += 1;
    }

    // The deployments of the artifacts that hold them, and the route
    // they take.
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
    let artefacts: Vec<Hrn> = artefact_ids
        .iter()
        .filter_map(|id| Hrn::from_string(id))
        .collect();
    if !artefacts.is_empty() {
        let deployed: Vec<String> = artefacts.iter().map(|node| node.to_string()).collect();
        let deployment_rows =
            relations_from_many_rows(graph, tenant, &artefacts, RelationType::DeployedAs).await?;
        record_route(
            &mut walk.route_relations,
            &mut walk.route_evidence,
            &mut walk.route_nodes,
            &deployed,
            &deployment_rows,
        );
        for row in &deployment_rows {
            walk.deployments.insert(row.from.clone());
            walk.subject_routes
                .entry(row.from.clone())
                .or_default()
                .push((row.relation_id.clone(), row.evidence_id.clone()));
        }
    }

    Ok(walk)
}

/// The relations that end at a node, of a kind, WITH their route step.
async fn relations_to_rows<C: Connection>(
    graph: &SurrealGraphStore<C>,
    tenant: &Hrn,
    node: &Hrn,
    relation: RelationType,
) -> Result<Vec<SupportStepRow>, String> {
    let mut response = graph
        .db()
        .query(
            "SELECT relation_id, evidence_id, from FROM type::table($table) \
             WHERE tenant_hrn = $tenant AND relation_type = $relation AND to = $value \
             AND (status IS NONE OR status = 'asserted')",
        )
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .bind(("relation", relation.as_str().to_string()))
        .bind(("value", node.to_string()))
        .await
        .map_err(|error| error.to_string())?;
    response
        .take(0)
        .map_err(|error| error.to_string())
}

/// The relations that end at MANY nodes, of a kind, WITH their step.
async fn relations_to_many_rows<C: Connection>(
    graph: &SurrealGraphStore<C>,
    tenant: &Hrn,
    nodes: &[Hrn],
    relation: RelationType,
) -> Result<Vec<SupportStepRow>, String> {
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = nodes.iter().map(|node| node.to_string()).collect();
    let mut response = graph
        .db()
        .query(
            "SELECT relation_id, evidence_id, from FROM type::table($table) \
             WHERE tenant_hrn = $tenant AND relation_type = $relation AND to IN $ids \
             AND (status IS NONE OR status = 'asserted')",
        )
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .bind(("relation", relation.as_str().to_string()))
        .bind(("ids", ids))
        .await
        .map_err(|error| error.to_string())?;
    response
        .take(0)
        .map_err(|error| error.to_string())
}

/// The relations that start at MANY nodes, of a kind, WITH their step.
async fn relations_from_many_rows<C: Connection>(
    graph: &SurrealGraphStore<C>,
    tenant: &Hrn,
    nodes: &[Hrn],
    relation: RelationType,
) -> Result<Vec<SupportStepRow>, String> {
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = nodes.iter().map(|node| node.to_string()).collect();
    let mut response = graph
        .db()
        .query(
            "SELECT relation_id, evidence_id, from FROM type::table($table) \
             WHERE tenant_hrn = $tenant AND relation_type = $relation AND from IN $ids \
             AND (status IS NONE OR status = 'asserted')",
        )
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .bind(("relation", relation.as_str().to_string()))
        .bind(("ids", ids))
        .await
        .map_err(|error| error.to_string())?;
    response
        .take(0)
        .map_err(|error| error.to_string())
}

/// The subjects of an answer, from a walk: the artifacts with their
/// depth, the components the vulnerability is in at depth zero, each
/// with the path that explains it.
fn the_answer_subjects(walk: &ImpactWalk) -> Vec<SubjectPath> {
    let mut subjects: Vec<SubjectPath> = Vec::new();
    for (subject, depth) in &walk.artifacts {
        let depth = if *depth == usize::MAX { 1 } else { *depth };
        let edges = walk.subject_routes.get(subject);
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
    for component in &walk.affected_components {
        let edges = walk.subject_routes.get(component);
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

/// The read-only port, over the real view.
#[async_trait::async_trait]
impl<C: Connection> ImpactReader for SurrealImpactView<C> {
    async fn serve_impact(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactAnswer, ImpactReadError> {
        SurrealImpactView::serve_impact(self, tenant, advisory).await
    }
}
