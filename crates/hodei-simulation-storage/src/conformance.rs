//! La conformidad de la simulacion sobre el grafo de verdad.
//!
//! La suite de conformidad vive en el dominio, y este es el adaptador
//! que la corre contra el grafo real: el mismo fixture, las mismas
//! cuentas y los mismos escenarios que se corren en memoria. Si el
//! adaptador dijera otra cosa que el de memoria, el fallo es del
//! adaptador y no del escenario.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use hodei_evidence::EvidenceId;
use hodei_evidence::application::projector::evidence_hrn;
use hodei_evidence::domain::graph::{GraphChangeSet, GraphNode, GraphRelation, NodeKind};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence_storage::SurrealGraphStore;
use hodei_simulation::conformance::{ConformanceWorld, Fixture};
use hodei_simulation::hypothetical::RelationFact;
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use kernel::Hrn;
use surrealdb::Connection;

use crate::SurrealSimulationGraph;

/// El mundo de conformidad sobre el grafo real.
///
/// El fixture se escribe en el grafo --nodos, evidencia y relaciones-- y
/// a partir de ahi se lee: el escenario ve el mismo mundo que ve el
/// servicio, no una copia.
pub struct SurrealConformance<C: Connection> {
    store: Arc<SurrealGraphStore<C>>,
    graph: Arc<SurrealSimulationGraph<C>>,
    tenant: Hrn,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl<C: Connection> SurrealConformance<C> {
    /// El mundo de conformidad sobre un grafo y un evaluador.
    pub fn new(
        store: Arc<SurrealGraphStore<C>>,
        tenant: Hrn,
        decisions: Arc<dyn DecisionEvaluator>,
    ) -> Self {
        let graph = Arc::new(SurrealSimulationGraph::new(store.clone(), tenant.clone()));
        Self {
            store,
            graph,
            tenant,
            decisions,
        }
    }
}

#[async_trait]
impl<C: Connection> ConformanceWorld for SurrealConformance<C> {
    fn account(&self) -> String {
        self.tenant.account_id().to_string()
    }

    fn tenant(&self) -> Hrn {
        self.tenant.clone()
    }

    async fn prepare(&self, fixture: &Fixture) -> Result<(), String> {
        let now = Utc::now();
        let mut changes = GraphChangeSet::empty();

        // Los nodos del fixture.
        for node in &fixture.nodes {
            changes = changes.with_node(GraphNode::new(
                node.hrn.clone(),
                fixture.tenant.clone(),
                node.kind.clone(),
                now,
            ));
        }

        // Y los nodos de la evidencia: una relacion que dice que la
        // sostiene una evidencia que no existe no se escribe.
        for relation in &fixture.relations {
            let Some(evidence) = relation.evidence.as_ref() else {
                continue;
            };
            let id = EvidenceId::new(evidence.clone());
            changes = changes.with_node(GraphNode::new(
                evidence_hrn(&fixture.tenant, &id),
                fixture.tenant.clone(),
                NodeKind::Evidence,
                now,
            ));
        }

        // Y las relaciones.
        for relation in &fixture.relations {
            let evidence = EvidenceId::new(relation.evidence.clone().unwrap_or_default());
            changes = changes.with_relation(GraphRelation::new(
                relation.relation.clone(),
                relation.from.clone(),
                relation.to.clone(),
                fixture.tenant.clone(),
                evidence,
                now,
            ));
        }

        self.store
            .apply(changes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn relations(&self) -> Result<Vec<RelationFact>, String> {
        self.graph.all_relations().await
    }

    async fn generation(&self) -> Result<u64, String> {
        self.graph.generation().await
    }

    fn decisions(&self) -> Arc<dyn DecisionEvaluator> {
        self.decisions.clone()
    }
}
