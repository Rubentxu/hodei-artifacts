//! El fixture en memoria: el mundo que la conformidad necesita sin base
//! de datos.
//!
//! La simulacion se lee contra un grafo, y un grafo de verdad es un
//! grafo de verdad. Para comprobar que la simulacion se comporta igual
//! con cualquier adaptador hace falta un adaptador que no sea el de
//! verdad: este es, y son las mismas relaciones con las mismas reglas,
//! pero guardadas en memoria.
//!
//! Vive en el dominio y no en el almacen porque es lo que hace que la
//! suite de conformidad sea **portable**: el dominio la puede correr sin
//! base de datos, y un adaptador de verdad la corre con las mismas
//! cuentas.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::read_set::ReadSet;
use hodei_trust_policy::RequirementKind;
use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::decision::Outcome;
use kernel::Hrn;

use crate::conformance::{ConformanceWorld, Fixture};
use crate::diff::TheDecision;
use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use crate::ports::{DecisionEvaluator, SimulationGraph};

/// El grafo en memoria.
///
/// Guarda las relaciones y su generacion, y no mas. No tiene un metodo
/// de escritura en el puerto de la simulacion, y por eso simular contra
/// el no puede cambiar el mundo: el unico que puede moverlo es el que
/// lo prepara.
pub struct InMemoryGraph {
    relations: Mutex<Vec<RelationFact>>,
    generation: Mutex<u64>,
    account: String,
}

impl InMemoryGraph {
    /// Un grafo en memoria, con su cuenta.
    pub fn new(account: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            relations: Mutex::new(Vec::new()),
            generation: Mutex::new(1),
            account: account.into(),
        })
    }

    /// Las relaciones que tiene ahora mismo.
    pub fn the_relations(&self) -> Vec<RelationFact> {
        self.relations
            .lock()
            .expect("el grafo no esta envenenado")
            .clone()
    }

    /// La generacion que tiene ahora mismo.
    pub fn the_generation(&self) -> u64 {
        *self
            .generation
            .lock()
            .expect("la generacion no esta envenenada")
    }

    /// Deja el mundo como dice el fixture.
    pub fn set_the_world(&self, relations: Vec<RelationFact>, generation: u64) {
        *self.relations.lock().expect("el grafo no esta envenenado") = relations;
        *self
            .generation
            .lock()
            .expect("la generacion no esta envenenada") = generation;
    }
}

#[async_trait]
impl SimulationGraph for InMemoryGraph {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        Ok(self
            .the_relations()
            .into_iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .collect())
    }

    async fn relation_count(&self) -> Result<usize, String> {
        Ok(self.the_relations().len())
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        Ok(self.the_relations())
    }

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.the_generation())
    }

    fn account(&self) -> String {
        self.account.clone()
    }
}

/// El evaluador en memoria.
///
/// Es el evaluador mas simple que se puede escribir sin mentir: dice
/// DENY cuando la politica prohibe el riesgo establecido y el sujeto
/// tiene una vulnerabilidad critica y alcanzable, y ALLOW en el resto.
/// Lo que no sabe, no lo dice.
pub struct InMemoryDecisions;

#[async_trait]
impl DecisionEvaluator for InMemoryDecisions {
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
        _now: DateTime<Utc>,
    ) -> Result<TheDecision, String> {
        let exposed = facts.iter().any(|fact| {
            matches!(
                &fact.claim,
                HypotheticalClaim::Risk {
                    subject: who,
                    critical,
                    reachable,
                    ..
                } if who == subject && *critical && *reachable
            )
        });
        let denies = policy
            .requirements
            .iter()
            .any(|requirement| requirement.kind == RequirementKind::RiskEstablished);
        let outcome = if denies && exposed {
            Outcome::Deny
        } else {
            Outcome::Allow
        };
        Ok(TheDecision {
            outcome,
            policy: policy.clone(),
            read_set: ReadSet::empty(),
        })
    }

    async fn the_reads_of(&self, _subject: &Hrn) -> Result<ReadSet, String> {
        Ok(ReadSet::empty())
    }
}

/// El mundo de conformidad en memoria.
///
/// Es el adaptador de la suite contra un grafo que no es un grafo de
/// verdad: las mismas relaciones, las mismas reglas y ningun disco.
pub struct InMemoryConformance {
    graph: Arc<InMemoryGraph>,
    tenant: Hrn,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl InMemoryConformance {
    /// El mundo en memoria de una cuenta.
    pub fn new(account: impl Into<String>, tenant: Hrn) -> Self {
        let account = account.into();
        Self {
            graph: InMemoryGraph::new(account),
            tenant,
            decisions: Arc::new(InMemoryDecisions),
        }
    }

    /// El grafo en memoria, por si hay que mirarlo.
    pub fn the_graph(&self) -> Arc<InMemoryGraph> {
        self.graph.clone()
    }
}

#[async_trait]
impl ConformanceWorld for InMemoryConformance {
    fn account(&self) -> String {
        SimulationGraph::account(&*self.graph)
    }

    fn tenant(&self) -> Hrn {
        self.tenant.clone()
    }

    async fn prepare(&self, fixture: &Fixture) -> Result<(), String> {
        self.graph
            .set_the_world(fixture.relations.clone(), fixture.generation);
        Ok(())
    }

    async fn relations(&self) -> Result<Vec<RelationFact>, String> {
        Ok(self.graph.the_relations())
    }

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.graph.the_generation())
    }

    fn decisions(&self) -> Arc<dyn DecisionEvaluator> {
        self.decisions.clone()
    }
}
