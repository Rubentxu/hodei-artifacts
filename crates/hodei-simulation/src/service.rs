//! El servicio que hace la simulacion.
//!
//! El recorrido es el de una pregunta de "que pasaria si":
//!
//! 1. Se comprueban los **limites** de la simulacion, antes de hacer
//!    nada: una simulacion que no cabe no se ejecuta.
//! 2. Se **fija la base**: la cuenta, la generacion del grafo y las
//!    relaciones que se van a leer, con su sello.
//! 3. Se lee el estado real del grafo, **solo lectura**.
//! 4. Se calcula el **diff estructural**: las relaciones del estado real
//!    contra las del fork.
//! 5. Se calculan las **decisiones** de antes y de despues, para los
//!    sujetos que la simulacion toca, y se queda solo las que cambian.
//! 6. Se **comprueba la base otra vez**: si el mundo se ha movido, la
//!    simulacion se dice obsoleta en vez de devolver un diff sobre un
//!    mundo que ya no existe.
//!
//! Y en ningun momento se escribe. El grafo real se comprueba al final:
//! si el numero de relaciones es el mismo que al principio, el fork no
//! toco nada, y eso no es una promesa sino una comprobacion.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::base::{SimulationBase, the_seal_of};
use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::fork::SimulationFork;
use crate::hypothetical::RelationFact;
use crate::ports::{DecisionEvaluator, SimulationGraph};

/// Por que no se pudo simular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationError {
    /// La simulacion es mas grande de lo que permite.
    TooLarge(String),
    /// La simulacion ha vivido mas de lo que permite.
    Expired(String),
    /// El estado real no se pudo leer.
    Unavailable(String),
    /// La simulacion pide lo que no puede: sujetos de otra cuenta o un
    /// alcance vacio que daria una conclusion engañosa.
    NotAllowed(String),
    /// Una LECTURA que la decision necesita fallo. No se reemplaza por
    /// una coleccion vacia ni por un resultado: sin lo que hay que leer,
    /// la decision no se toma.
    ReadFailed(String),
    /// La base se movio mientras se evaluaba: el mundo contra el que se
    /// leyo ya no es el mundo de ahora.
    ///
    /// No es un fallo de la simulacion: es su respuesta. Un diff
    /// calculado sobre un mundo que nadie tiene es un diff que no dice
    /// la verdad, y se dice antes que devolverlo.
    StaleBase {
        /// Que se movio: la generacion del grafo, las relaciones leidas
        /// o el numero de relaciones.
        source: String,
        /// Lo que se esperaba.
        expected: String,
        /// Lo que se encontro.
        found: String,
    },
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(message) => write!(formatter, "the simulation is too large: {message}"),
            Self::Expired(message) => write!(formatter, "the simulation expired: {message}"),
            Self::Unavailable(message) => {
                write!(formatter, "the state could not be read: {message}")
            }
            Self::NotAllowed(message) => {
                write!(formatter, "the simulation is not allowed: {message}")
            }
            Self::ReadFailed(message) => {
                write!(
                    formatter,
                    "a read that the decision needs failed: {message}"
                )
            }
            Self::StaleBase {
                source,
                expected,
                found,
            } => write!(
                formatter,
                "the base of the simulation is stale: {source} was {expected} and it is now {found}"
            ),
        }
    }
}

/// El servicio de la simulacion.
pub struct SimulationService {
    graph: Arc<dyn SimulationGraph>,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl SimulationService {
    /// El servicio sobre un grafo y un evaluador.
    pub fn new(graph: Arc<dyn SimulationGraph>, decisions: Arc<dyn DecisionEvaluator>) -> Self {
        Self { graph, decisions }
    }

    /// Las relaciones del grafo real.
    ///
    /// Lo expone el servicio para que se pueda comprobar que el grafo no
    /// ha cambiado: una simulacion que no deja rastro es lo que se quiere,
    /// y "no deja rastro" se comprueba, no se promete.
    pub async fn graph_relations(&self) -> Result<Vec<RelationFact>, SimulationError> {
        let mut response = self
            .graph
            .all_relations()
            .await
            .map_err(SimulationError::Unavailable)?;
        response.sort_by_key(|relation| relation.identity());
        Ok(response)
    }

    /// Simula y devuelve el diff.
    ///
    /// El `now` es el reloj de la simulacion: el mismo instante para
    /// todo el recorrido, y el que llega al nucleo de decision. No se
    /// lee el reloj de pared en ningun sitio, y por eso la misma base y
    /// los mismos hechos dan el mismo diff.
    pub async fn simulate(
        &self,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<SimulationDiff, SimulationError> {
        // 1. Los limites, antes de nada.
        fork.check_the_limits(now).map_err(|error| match error {
            crate::SimulationError::TooLarge(message) => SimulationError::TooLarge(message),
            crate::SimulationError::Expired(message) => SimulationError::Expired(message),
            other => other,
        })?;

        // 2. La cuenta: la base de OTRA cuenta no es una base. El grafo
        // es de una cuenta y el fork declara otra, y una simulacion no
        // puede ser de las dos a la vez.
        let account = self.graph.account();
        if account != fork.base.account {
            return Err(SimulationError::NotAllowed(format!(
                "the base is about account {} and the graph is about account {account}",
                fork.base.account
            )));
        }

        // 3. La generacion: si el fork se preparo contra otra generacion,
        // el mundo ya no es el que el fork mia, y se dice antes de
        // leer nada.
        let generation = self
            .graph
            .generation()
            .await
            .map_err(SimulationError::Unavailable)?;
        if let Some(expected) = fork.base.generation
            && expected != generation
        {
            return Err(SimulationError::StaleBase {
                source: "graph".to_string(),
                expected: expected.to_string(),
                found: generation.to_string(),
            });
        }

        // 4. El numero de relaciones de antes: el aislamiento se comprueba
        // contra el.
        let relations_before = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;

        // 5. El diff estructural.
        let subjects = fork.affected_subjects();
        let real = self.the_relations_of(&subjects).await?;

        // Y la base, con lo que se ha leido: el manifiesto de lo leido y
        // el sello de su contenido.
        let base = SimulationBase::new(account, now)
            .with_generation("graph", generation)
            .with_relations(&real)
            .with_policy(&fork.base_policy);
        let seal_before = base.seal.clone();

        let forked = fork.relations_with(&real);
        let mut structural = StructuralDiff::between(&real, &forked);

        // Los sujetos afectados son los del fork y los del diff: una
        // simulacion que solo cambia la politica no cambia ninguna
        // relacion, y aun asi hay sujetos a los que les afecta.
        for subject in &subjects {
            if !structural.affected_subjects.contains(subject) {
                structural.affected_subjects.push(subject.clone());
            }
        }
        // El HRN no tiene orden natural, y el orden estable es por su
        // forma: el diff no depende del orden en que se leyó.
        structural
            .affected_subjects
            .sort_by_key(|subject| subject.to_string());

        // Y los despliegues de los sujetos a los que el cambio llega.
        structural.affected_deployments = forked
            .iter()
            .filter(|relation| {
                relation.relation == hodei_evidence::domain::graph::RelationType::DeployedAs
                    && structural.affected_subjects.contains(&relation.from)
            })
            .map(|relation| relation.to.clone())
            .collect();
        structural
            .affected_deployments
            .sort_by_key(|deployment| deployment.to_string());

        // 6. Las decisiones: las de antes con la politica de antes, y las
        // de despues con la del fork. El instante es el de la
        // simulacion, no el del reloj de pared.
        let mut decisions: Vec<DecisionChange> = Vec::new();
        for subject in &subjects {
            let before = self
                .decisions
                .decide(subject, &fork.base_policy, &[], now)
                .await
                .map_err(SimulationError::ReadFailed)?;
            let after = self
                .decisions
                .decide(subject, fork.effective_policy(), &fork.facts, now)
                .await
                .map_err(SimulationError::ReadFailed)?;
            let change = DecisionChange {
                subject: subject.clone(),
                before: before.outcome,
                after: after.outcome,
                policy_version_before: before.policy.version.clone(),
                policy_version_after: after.policy.version.clone(),
            };
            if change.is_a_change() {
                decisions.push(change);
            }
        }

        // 7. La comprobacion final: el mundo no se ha movido.
        //
        // La generacion dice si la historia del grafo ha avanzado, y el
        // sello de contenido dice si lo que se leyo es lo mismo. Los dos
        // hacen falta: el numero de relaciones no los sustituye, porque
        // una relacion sustituida por otra deja el numero igual.
        //
        // Y el estado de "despues" se toma lo ULTIMO, con las tres
        // lecturas seguidas: si se leyera la generacion antes que las
        // relaciones, un cambio que cae entre las dos lecturas pasaria
        // desapercibido, y el sello se compararia contra un mundo que
        // ya no es el que se evaluo.
        let real_after = self.the_relations_of(&subjects).await?;
        let seal_after = the_seal_of(&real_after);

        let generation_after = self
            .graph
            .generation()
            .await
            .map_err(SimulationError::Unavailable)?;
        if base.is_the_generation_stale(generation_after) {
            return Err(SimulationError::StaleBase {
                source: "graph".to_string(),
                expected: generation.to_string(),
                found: generation_after.to_string(),
            });
        }

        let relations_after = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;
        if relations_after != relations_before {
            return Err(SimulationError::StaleBase {
                source: "relation count".to_string(),
                expected: relations_before.to_string(),
                found: relations_after.to_string(),
            });
        }

        if base.is_the_content_stale(&seal_after) {
            return Err(SimulationError::StaleBase {
                source: "relations".to_string(),
                expected: seal_before,
                found: seal_after,
            });
        }

        Ok(SimulationDiff {
            structural,
            decisions,
            base,
        })
    }

    /// Las relaciones de los sujetos, en orden canonico.
    async fn the_relations_of(
        &self,
        subjects: &[kernel::Hrn],
    ) -> Result<Vec<RelationFact>, SimulationError> {
        let mut relations: Vec<RelationFact> = Vec::new();
        for subject in subjects {
            for relation in self
                .graph
                .relations_of(subject)
                .await
                .map_err(SimulationError::Unavailable)?
            {
                if !relations.contains(&relation) {
                    relations.push(relation);
                }
            }
        }
        relations.sort_by_key(|relation| relation.identity());
        Ok(relations)
    }
}
