//! El servicio que hace la simulacion.
//!
//! El recorrido es el de una pregunta de "que pasaria si":
//!
//! 1. Se comprueban los **limites** de la simulacion, antes de hacer
//!    nada: una simulacion que no cabe no se ejecuta.
//! 2. Se lee el estado real del grafo, **solo lectura**.
//! 3. Se calcula el **diff estructural**: las relaciones del estado real
//!    contra las del fork.
//! 4. Se calculan las **decisiones** de antes y de despues, para los
//!    sujetos que la simulacion toca, y se queda solo las que cambian.
//!
//! Y en ningun momento se escribe. El grafo real se comprueba al final:
//! si el numero de relaciones es el mismo que al principio, el fork no
//! toco nada, y eso no es una promesa sino una comprobacion.

use std::sync::Arc;

use chrono::{DateTime, Utc};

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
                write!(formatter, "a read that the decision needs failed: {message}")
            }
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

        // 2. El numero de relaciones de antes: el aislamiento se comprueba
        // contra el.
        let relations_before = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;

        // 3. El diff estructural.
        let subjects = fork.affected_subjects();
        let mut real: Vec<RelationFact> = Vec::new();
        for subject in &subjects {
            let relations = self
                .graph
                .relations_of(subject)
                .await
                .map_err(SimulationError::Unavailable)?;
            for relation in relations {
                if !real.contains(&relation) {
                    real.push(relation);
                }
            }
        }
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

        // Y los despliegues de los sujetos a los que el cambio llega.
        structural.affected_deployments = forked
            .iter()
            .filter(|relation| {
                relation.relation == hodei_evidence::domain::graph::RelationType::DeployedAs
                    && structural.affected_subjects.contains(&relation.from)
            })
            .map(|relation| relation.to.clone())
            .collect();

        // 4. Las decisiones: las de antes con la politica de antes, y las
        // de despues con la del fork.
        let mut decisions: Vec<DecisionChange> = Vec::new();
        for subject in &subjects {
            let before = self
                .decisions
                .decide(subject, &fork.base_policy, &[])
                .await
                .map_err(SimulationError::ReadFailed)?;
            let after = self
                .decisions
                .decide(subject, fork.effective_policy(), &fork.facts)
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

        // Y el aislamiento, comprobado y no prometido.
        let relations_after = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;
        if relations_after != relations_before {
            return Err(SimulationError::Unavailable(format!(
                "the graph changed while simulating: {relations_before} relations before and \
                 {relations_after} after"
            )));
        }

        Ok(SimulationDiff {
            structural,
            decisions,
        })
    }
}
