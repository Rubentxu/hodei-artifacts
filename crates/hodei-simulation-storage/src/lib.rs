//! El almacen de la simulacion: los adaptadores que leen el estado real.
//!
//! El grafo real se lee y **no se escribe**: el puerto
//! [`SimulationGraph`] no tiene un metodo de escritura, y por eso no
//! existe por donde escribir. El adaptador de aqui es la pieza que
//! cumple esa promesa en el mundo real, y por eso ademas dice la
//! VERSION del grafo que lee: una simulacion que no dice sobre que
//! generacion del mundo se calculo no se puede auditar.

pub mod evaluator;

pub use evaluator::CapsuleDecisionEvaluator;

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::ports::graph::GraphQuery;
use hodei_simulation::hypothetical::RelationFact;
use hodei_simulation::ports::SimulationGraph;
use kernel::Hrn;
use surrealdb::Connection;

/// El grafo real, leido para simular.
pub struct SurrealSimulationGraph<C: Connection> {
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    tenant: Hrn,
}

impl<C: Connection> SurrealSimulationGraph<C> {
    /// El grafo de una cuenta.
    pub fn new(graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>, tenant: Hrn) -> Self {
        Self { graph, tenant }
    }
}

#[async_trait]
impl<C: Connection> SimulationGraph for SurrealSimulationGraph<C> {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        let mut facts = Vec::new();

        // Las relaciones que parten del sujeto.
        for relation in self
            .graph
            .relations_from(&self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?
        {
            facts.push(
                RelationFact::new(relation.relation, relation.from, relation.to)
                    .with_evidence(relation.evidence_id.to_string()),
            );
        }

        // Y las que llegan a el.
        for relation in self
            .graph
            .relations_to(&self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?
        {
            let fact = RelationFact::new(relation.relation, relation.from, relation.to)
                .with_evidence(relation.evidence_id.to_string());
            if !facts.contains(&fact) {
                facts.push(fact);
            }
        }

        Ok(facts)
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        let mut response = self
            .graph
            .db()
            .query(
                "SELECT from, to, relation_type, evidence_id FROM type::table($table) WHERE \
                 tenant_hrn = $tenant",
            )
            .bind(("table", self.graph.relation_table_name()))
            .bind(("tenant", self.tenant.to_string()))
            .await
            .map_err(|error| error.to_string())?;
        let rows: Vec<RelationRow> = response.take(0).map_err(|error| error.to_string())?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let relation =
                    hodei_evidence::domain::graph::RelationType::parse(&row.relation_type)?;
                let from = Hrn::from_string(&row.from)?;
                let to = Hrn::from_string(&row.to)?;
                let mut fact = RelationFact::new(relation, from, to);
                if let Some(evidence) = row.evidence_id {
                    fact = fact.with_evidence(evidence);
                }
                Some(fact)
            })
            .collect())
    }

    async fn relation_count(&self) -> Result<usize, String> {
        let mut response = self
            .graph
            .db()
            .query("SELECT from FROM type::table($table) WHERE tenant_hrn = $tenant")
            .bind(("table", self.graph.relation_table_name()))
            .bind(("tenant", self.tenant.to_string()))
            .await
            .map_err(|error| error.to_string())?;
        let rows: Vec<CountRow> = response.take(0).map_err(|error| error.to_string())?;
        Ok(rows.len())
    }

    async fn generation(&self) -> Result<u64, String> {
        self.graph.generation(&self.tenant).await
    }

    fn account(&self) -> String {
        self.tenant.account_id().to_string()
    }
}

/// La relacion de una fila.
#[derive(Debug, Clone, serde::Deserialize)]
struct RelationRow {
    /// El nodo del que parte.
    from: String,
    /// El nodo al que llega.
    to: String,
    /// El tipo de la relacion.
    relation_type: String,
    /// La evidencia que la sostiene, si la fila la trae.
    #[serde(default)]
    evidence_id: Option<String>,
}

/// La fila de la cuenta.
#[derive(Debug, Clone, serde::Deserialize)]
struct CountRow {
    /// El nodo del que parte la relacion.
    #[allow(dead_code)]
    from: String,
}
