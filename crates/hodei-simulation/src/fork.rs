//! El fork: un estado aislado para preguntar "que pasaria si".
//!
//! El fork **no copia** el estado real: lo **superpone**. El estado real
//! se lee y no se toca, y el fork son los hechos hipoteticos y nada mas.
//! Esa es la decision del SPIKE-008, y su razon es que una copia cuesta
//! lo que cuesta el estado (156 s de proyeccion y 656.6 s de
//! reconstruccion a la escala de referencia) mientras que una superposicion
//! cuesta lo que cuestan los hechos hipoteticos, que son pocos.
//!
//! Y la superposicion es **aislada por construccion**: el fork es un
//! VALOR. No hay un puerto de escritura que lo alcance, y por eso
//! simular no puede cambiar el grafo real.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};

/// El punto del estado real del que sale el fork.
///
/// El checkpoint dice **de donde** se parte, y no lleva el estado: el
/// estado se lee del almacen real, que es lo que se quiere (la verdad de
/// ahora, no una copia de la verdad de hace rato).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// El identificador del checkpoint.
    pub id: String,
    /// La cuenta a la que pertenece.
    pub account: String,
    /// El momento del checkpoint.
    pub at: DateTime<Utc>,
    /// El numero de eventos del ledger en ese punto, si se sabe.
    ///
    /// Sirve para comprobar que el grafo real no ha cambiado: un fork no
    /// deja ni una sombra, y el contador lo dice.
    pub ledger_position: Option<u64>,
}

impl Checkpoint {
    /// Un checkpoint.
    pub fn new(
        id: impl Into<String>,
        account: impl Into<String>,
        at: DateTime<Utc>,
        ledger_position: Option<u64>,
    ) -> Self {
        Self {
            id: id.into(),
            account: account.into(),
            at,
            ledger_position,
        }
    }
}

/// Los limites de una simulacion.
///
/// Una simulacion es un recurso, y un recurso sin limite se come la
/// maquina. Los limites son **declarados**: el que simula sabe de
/// antemano cuanto puede gastar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationLimits {
    /// Cuantos hechos hipoteticos puede tener la simulacion.
    pub max_hypotheticals: usize,
    /// Cuantos sujetos puede tocar.
    pub max_affected_subjects: usize,
    /// Cuanto vive la simulacion.
    pub max_lifetime: chrono::Duration,
}

impl Default for SimulationLimits {
    fn default() -> Self {
        Self {
            max_hypotheticals: 100,
            max_affected_subjects: 1_000,
            max_lifetime: chrono::Duration::hours(24),
        }
    }
}

/// El fork aislado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationFork {
    /// El identificador de la simulacion.
    pub id: String,
    /// De donde se parte.
    pub base: Checkpoint,
    /// Los hechos hipoteticos, superpuestos al estado real.
    pub facts: Vec<Hypothetical>,
    /// Los sujetos DECLARADOS que la simulacion toca.
    ///
    /// Una simulacion que solo cambia la politica no trae hechos, y los
    /// sujetos no se infieren de lo que no hay: se DECLARAN, y quien
    /// declara responde de que puede simularlos.
    #[serde(default)]
    pub subjects: Vec<Hrn>,
    /// La politica que estaria en vigor, si la simulacion cambia la.
    pub policy: Option<TrustPolicy>,
    /// La politica que esta en vigor ahora, en el estado real.
    ///
    /// Es la que decide "antes", y sin ella el diff de decisiones no
    /// tendria contra que comparar.
    pub base_policy: TrustPolicy,
    /// Los limites.
    pub limits: SimulationLimits,
    /// Cuando se creo.
    pub created_at: DateTime<Utc>,
}

impl SimulationFork {
    /// Un fork vacio, sin hechos hipoteticos.
    pub fn new(
        id: impl Into<String>,
        base: Checkpoint,
        base_policy: TrustPolicy,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            base,
            facts: Vec::new(),
            subjects: Vec::new(),
            policy: None,
            base_policy,
            limits: SimulationLimits::default(),
            created_at,
        }
    }

    /// Declara los sujetos que la simulacion toca.
    ///
    /// Es la seleccion EXPLICITA: los sujetos que se simulan son los que
    /// se declaran mas los que traen los hechos, y una simulacion que
    /// solo cambia la politica necesita sujetos declarados, porque de
    /// los hechos que no hay no se infiere nada.
    pub fn with_subjects(mut self, subjects: Vec<Hrn>) -> Self {
        for subject in subjects {
            if !self.subjects.contains(&subject) {
                self.subjects.push(subject);
            }
        }
        self
    }

    /// La politica que estaria en vigor en el fork.
    ///
    /// Es la del fork si la simulacion cambia la politica, y la del estado
    /// real si no: una simulacion que no toca la politica se evalua con
    /// la politica de ahora.
    pub fn effective_policy(&self) -> &TrustPolicy {
        self.policy.as_ref().unwrap_or(&self.base_policy)
    }

    /// Superpone un hecho hipotetico.
    pub fn with(mut self, fact: Hypothetical) -> Self {
        self.facts.push(fact);
        self
    }

    /// Superpone una politica hipotetica.
    pub fn with_policy(mut self, policy: TrustPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Los sujetos que la simulacion toca.
    ///
    /// Son los que su decision puede cambiar, y por tanto los que el diff
    /// de decisiones tiene que mirar.
    pub fn affected_subjects(&self) -> Vec<Hrn> {
        let mut subjects: Vec<Hrn> = self.subjects.clone();
        for fact in &self.facts {
            for subject in fact.claim.subjects() {
                if !subjects.contains(&subject) {
                    subjects.push(subject);
                }
            }
        }
        subjects
    }

    /// Las relaciones que el fork anade al estado real.
    pub fn added_relations(&self) -> Vec<RelationFact> {
        self.facts
            .iter()
            .filter_map(|fact| match &fact.claim {
                HypotheticalClaim::Relation {
                    relation, from, to, ..
                } => Some(RelationFact::new(
                    relation.clone(),
                    from.clone(),
                    to.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    /// Las relaciones que el fork hace desaparecer del estado real.
    pub fn removed_relations(&self) -> Vec<RelationFact> {
        self.facts
            .iter()
            .filter_map(|fact| match &fact.claim {
                HypotheticalClaim::Relation { replaces, .. } => replaces.clone(),
                _ => None,
            })
            .collect()
    }

    /// La relacion que se consulta para un sujeto.
    ///
    /// La del fork es la del estado real MAS las hipoteticas MINUS las que
    /// el fork sustituye. Y la substitucion se comprueba por identidad:
    /// dos relaciones con la misma identidad son la misma relacion.
    pub fn relations_with(&self, real: &[RelationFact]) -> Vec<RelationFact> {
        let removed: Vec<String> = self
            .removed_relations()
            .iter()
            .map(RelationFact::identity)
            .collect();
        let mut relations: Vec<RelationFact> = real
            .iter()
            .filter(|relation| !removed.contains(&relation.identity()))
            .cloned()
            .collect();
        for added in self.added_relations() {
            if !relations
                .iter()
                .any(|one| one.identity() == added.identity())
            {
                relations.push(added);
            }
        }
        relations
    }

    /// Comprueba que la simulacion cabe en sus limites.
    pub fn check_the_limits(&self, now: DateTime<Utc>) -> Result<(), crate::SimulationError> {
        if self.facts.len() > self.limits.max_hypotheticals {
            return Err(crate::SimulationError::TooLarge(format!(
                "the simulation has {} hypotheticals and the limit is {}",
                self.facts.len(),
                self.limits.max_hypotheticals
            )));
        }
        let subjects = self.affected_subjects();
        if subjects.len() > self.limits.max_affected_subjects {
            return Err(crate::SimulationError::TooLarge(format!(
                "the simulation touches {} subjects and the limit is {}",
                subjects.len(),
                self.limits.max_affected_subjects
            )));
        }
        if now > self.created_at + self.limits.max_lifetime {
            return Err(crate::SimulationError::Expired(format!(
                "the simulation was created at {} and lives {}",
                self.created_at, self.limits.max_lifetime
            )));
        }

        // La admision de los sujetos: los de OTRA cuenta no entran, y la
        // comparacion es por componente canonico del HRN, no por texto.
        // Un fork sin NINGUN sujeto no es una simulacion completa sin
        // afectados: es una conclusion engañosa, y no se produce.
        let subjects = self.affected_subjects();
        if subjects.is_empty() {
            return Err(crate::SimulationError::NotAllowed(
                "the simulation declares no subjects: a fork that touches nobody is not a \
                 complete simulation, it is a misleading answer"
                    .to_string(),
            ));
        }
        for subject in &subjects {
            if subject.account_id() != self.base.account {
                return Err(crate::SimulationError::NotAllowed(format!(
                    "the subject {subject} belongs to account {} and the simulation is about {}",
                    subject.account_id(),
                    self.base.account
                )));
            }
        }
        Ok(())
    }

    /// La relacion de un sujeto, por su relacion.
    pub fn relations_of(&self, subject: &Hrn, real: &[RelationFact]) -> Vec<RelationFact> {
        self.relations_with(real)
            .into_iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .collect()
    }

    /// Las relaciones de un tipo, del fork.
    pub fn relations_of_kind(
        &self,
        real: &[RelationFact],
        relation: RelationType,
    ) -> Vec<RelationFact> {
        self.relations_with(real)
            .into_iter()
            .filter(|one| one.relation == relation)
            .collect()
    }
}
