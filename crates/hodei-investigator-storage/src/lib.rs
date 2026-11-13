//! El recuperador real: el subgrafo del sujeto, con su evidencia y sus
//! caminos.
//!
//! El recuperador lee **tres** cosas del sujeto, y las tres son evidencia
//! con su identificador:
//!
//! - las **piezas de evidencia** del sujeto, con su contenido real (que
//!   es texto de un tercero, y por tanto dato);
//! - las **relaciones del grafo** que lo tocan, con el camino;
//! - y la **evidencia de alcanzabilidad**, si la hay.
//!
//! Y todo lo que devuelve cabe en el alcance: si no cabe, el
//! investigador lo rechaza antes de preguntarle al modelo.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::GraphQuery;
use hodei_evidence::ports::{ContentStore, EvidenceStore};
use hodei_investigator::investigator::{EvidenceRetriever, ReachabilityFacts};
use hodei_investigator::scope::{RetrievalScope, ScopedFact, TrustLevel};
use kernel::Hrn;
use surrealdb::Connection;

/// El recuperador sobre el grafo y la evidencia reales.
pub struct GraphEvidenceRetriever<C: Connection> {
    evidence: Arc<dyn EvidenceStore>,
    content: Arc<dyn ContentStore>,
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    reachability: Option<Arc<dyn ReachabilityFacts>>,
    tenant: Hrn,
}

impl<C: Connection> GraphEvidenceRetriever<C> {
    /// El recuperador sobre un almacen de evidencia, un almacen de
    /// contenido y un grafo.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        content: Arc<dyn ContentStore>,
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
        tenant: Hrn,
    ) -> Self {
        Self {
            evidence,
            content,
            graph,
            reachability: None,
            tenant,
        }
    }

    /// El recuperador que tambien sabe de alcanzabilidad.
    pub fn with_reachability(mut self, reachability: Arc<dyn ReachabilityFacts>) -> Self {
        self.reachability = Some(reachability);
        self
    }

    /// Las piezas de evidencia del sujeto, con su contenido real.
    async fn the_evidence(&self, account: &str, subject: &Hrn) -> Result<Vec<ScopedFact>, String> {
        let records = self
            .evidence
            .evidence_of_subject(account, subject)
            .await
            .map_err(|error| error.to_string())?;

        let mut facts = Vec::new();
        for record in records {
            // El contenido es el texto del tercero: la descripcion del
            // SBOM, el README, el mensaje del escaner. Se lee entero, y
            // se entrega como DATO.
            let text = match self.content.get(&record.content().digest).await {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
                _ => format!(
                    "the {} of the producer {}",
                    record.evidence_type(),
                    record.producer().id
                ),
            };
            facts.push(ScopedFact {
                evidence_id: record.id().to_string(),
                text,
                path: vec![subject.clone()],
                kind: NodeKind::Evidence,
                trust: TrustLevel::Untrusted,
            });
        }
        Ok(facts)
    }

    /// Las relaciones del grafo que tocan al sujeto, con su camino.
    async fn the_relations(&self, subject: &Hrn) -> Result<Vec<ScopedFact>, String> {
        let mut facts = Vec::new();

        let mut relations = self
            .graph
            .relations_from(&self.tenant, subject)
            .await
            .map_err(|error| error.to_string())?;
        relations.extend(
            self.graph
                .relations_to(&self.tenant, subject)
                .await
                .map_err(|error| error.to_string())?,
        );

        for relation in relations {
            let path = if relation.from == *subject {
                vec![subject.clone(), relation.to.clone()]
            } else {
                vec![subject.clone(), relation.from.clone()]
            };
            facts.push(ScopedFact {
                evidence_id: relation.evidence_id.to_string(),
                text: format!(
                    "{} {} {}",
                    relation.from,
                    relation.relation.as_str(),
                    relation.to
                ),
                path,
                kind: NodeKind::Artifact,
                trust: TrustLevel::Untrusted,
            });
        }
        Ok(facts)
    }
}

#[async_trait]
impl<C: Connection> EvidenceRetriever for GraphEvidenceRetriever<C> {
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<Vec<ScopedFact>, String> {
        let mut facts = self.the_evidence(&scope.account, &scope.subject).await?;
        facts.extend(self.the_relations(&scope.subject).await?);
        if let Some(reachability) = &self.reachability {
            facts.extend(reachability.facts_of(&scope.subject).await?);
        }

        // Y el alcance se respeta aqui tambien: un recuperador que
        // devuelve de mas no puede colar nada, porque el investigador lo
        // comprueba otra vez.
        facts.truncate(scope.max_nodes);
        Ok(facts)
    }
}
