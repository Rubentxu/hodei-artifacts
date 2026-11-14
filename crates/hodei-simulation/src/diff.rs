//! El diff: lo que aparece, lo que desaparece y lo que cambia.
//!
//! El diff tiene que parecerse al diff de git: se ve que relaciones
//! aparecen y que relaciones desaparecen, y que decisiones cambian, sin
//! tener que desplegar nada para averiguarlo.

use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::decision::Outcome;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::hypothetical::RelationFact;

/// El diff de las relaciones del grafo.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StructuralDiff {
    /// Las relaciones que aparecen.
    pub appeared: Vec<RelationFact>,
    /// Las relaciones que desaparecen.
    pub disappeared: Vec<RelationFact>,
    /// Los sujetos a los que el cambio llega.
    pub affected_subjects: Vec<Hrn>,
    /// Los despliegues de esos sujetos.
    ///
    /// Un despliegue es donde duele el cambio: un artefacto que deja de
    /// ser promovible duele en el despliegue que lo ejecuta, y el diff
    /// tiene que decirlo.
    pub affected_deployments: Vec<Hrn>,
}

impl StructuralDiff {
    /// Un diff vacio.
    pub fn empty() -> Self {
        Self::default()
    }

    /// El diff entre dos conjuntos de relaciones.
    ///
    /// Las relaciones se comparan por su IDENTIDAD (el tipo y los dos
    /// extremos): dos relaciones con la misma identidad son la misma
    /// relacion, y compararlas por sus campos seria comparar dos veces la
    /// misma fila.
    pub fn between(before: &[RelationFact], after: &[RelationFact]) -> Self {
        let before_ids: Vec<String> = before.iter().map(RelationFact::identity).collect();
        let after_ids: Vec<String> = after.iter().map(RelationFact::identity).collect();

        let appeared: Vec<RelationFact> = after
            .iter()
            .filter(|relation| !before_ids.contains(&relation.identity()))
            .cloned()
            .collect();
        let disappeared: Vec<RelationFact> = before
            .iter()
            .filter(|relation| !after_ids.contains(&relation.identity()))
            .cloned()
            .collect();

        let mut affected: Vec<Hrn> = Vec::new();
        for relation in appeared.iter().chain(disappeared.iter()) {
            for subject in [&relation.from, &relation.to] {
                if !affected.contains(subject) {
                    affected.push(subject.clone());
                }
            }
        }

        Self {
            appeared,
            disappeared,
            affected_subjects: affected,
            affected_deployments: Vec::new(),
        }
    }

    /// Si el diff no dice nada.
    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.disappeared.is_empty()
    }
}

/// Una decision que cambia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionChange {
    /// El sujeto cuya decision cambia.
    pub subject: Hrn,
    /// Lo que se decidia antes.
    pub before: Outcome,
    /// Lo que se decidiria ahora.
    pub after: Outcome,
    /// La version de la politica de antes.
    pub policy_version_before: String,
    /// La version de la politica de ahora.
    pub policy_version_after: String,
}

impl DecisionChange {
    /// La decision cambia de verdad.
    ///
    /// Una decision que sigue igual, aunque se haya vuelto a evaluar, no
    /// es un cambio: y un diff lleno de ruido no es un diff.
    pub fn is_a_change(&self) -> bool {
        self.before != self.after
    }
}

/// El diff completo de la simulacion.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SimulationDiff {
    /// El diff de las relaciones.
    pub structural: StructuralDiff,
    /// Las decisiones que cambian.
    pub decisions: Vec<DecisionChange>,
    /// La base sobre la que se calculo: el mundo que el diff describe.
    ///
    /// Sin ella el diff es una respuesta sin el mundo al que se refiere:
    /// no dice sobre que version de la verdad se calculo, y por eso no
    /// se puede auditar.
    #[serde(default)]
    pub base: crate::base::SimulationBase,
}

impl SimulationDiff {
    /// Un diff vacio.
    pub fn empty() -> Self {
        Self::default()
    }

    /// El diff dice que nada cambia.
    pub fn is_empty(&self) -> bool {
        self.structural.is_empty() && self.decisions.iter().all(|change| !change.is_a_change())
    }
}

/// La decision de un sujeto, con la politica que la produjo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheDecision {
    /// Lo que se decidio.
    pub outcome: Outcome,
    /// La politica que lo decidio.
    pub policy: TrustPolicy,
}
