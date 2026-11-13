//! Los hechos hipoteticos de una simulacion.
//!
//! Un hecho hipotetico es un hecho que **no ha pasado**: es lo que se
//! superpone al estado real para preguntar "que pasaria si". Y la
//! diferencia entre un hecho real y uno hipotetico no es una convencion:
//! es una **etiqueta** que viaja con el hecho, y una etiqueta que se puede
//! perder es una etiqueta que no existe.
//!
//! Por eso la etiqueta es parte del tipo, y por eso las consultas
//! normales del grafo nunca ven un hecho hipotetico: no estan en el
//! grafo.

use chrono::{DateTime, Utc};
use hodei_evidence::EvidenceId;
use hodei_evidence::domain::graph::RelationType;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// La etiqueta que dice que un hecho es de una simulacion.
///
/// La etiqueta lleva el identificador de la simulacion, para que un hecho
/// de una simulacion no se confunda con el de otra, y para que se pueda
/// auditar despues que se simulo y que.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationTag {
    /// El identificador de la simulacion.
    pub simulation: String,
    /// El momento en que se creo el hecho.
    pub created_at: DateTime<Utc>,
}

impl SimulationTag {
    /// La etiqueta de una simulacion.
    pub fn new(simulation: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Self {
            simulation: simulation.into(),
            created_at,
        }
    }
}

/// Un hecho que se superpone al estado real.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hypothetical {
    /// La etiqueta: este hecho es de una simulacion.
    pub tag: SimulationTag,
    /// Lo que dice.
    pub claim: HypotheticalClaim,
}

/// Lo que dice un hecho hipotetico.
///
/// Las variantes tienen tamaños muy distintos, y la diferencia no importa
/// aqui: una simulacion tiene **como mucho cien hechos hipoteticos** (el
/// limite esta declarado y se comprueba), de modo que el desperdicio
/// seria de unos cuantos kilobytes. Empaquetarlas todas en cajas
/// haria el codigo peor para no arreglar nada.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypotheticalClaim {
    /// Una relacion que existiria.
    Relation {
        /// De que relacion se trata.
        relation: RelationType,
        /// El nodo del que parte.
        from: Hrn,
        /// El nodo al que llega.
        to: Hrn,
        /// La relacion que SUSTITUYE, si sustituye a alguna.
        ///
        /// Sustituir es lo que hace que una relacion desaparezca: sin
        /// esto, un cambio de dependencia solo anadiria y el diff no
        /// diria nada de lo que desaparece.
        replaces: Option<RelationFact>,
    },
    /// Una pieza de evidencia queiria, con lo que dice de su sujeto.
    Evidence {
        /// La evidencia.
        evidence: EvidenceId,
        /// De que pieza de evidencia se trata.
        kind: String,
        /// El sujeto del que habla.
        subject: Hrn,
        /// Si la evidencia estaria verificada.
        verified: bool,
    },
    /// Una politica que estaria en vigor.
    ///
    /// La politica va en una caja porque es mucho mas grande que las
    /// demas variantes, y una diferencia de tamaño tan grande convierte
    /// cada hecho hipotetico en una wasted memoria.
    Policy {
        /// La politica.
        policy: Box<TrustPolicy>,
    },
}

impl HypotheticalClaim {
    /// Los sujetos que el hecho toca.
    ///
    /// Son los sujetos cuya decision puede cambiar, y por tanto los que
    /// el diff de decisiones tiene que mirar.
    pub fn subjects(&self) -> Vec<Hrn> {
        match self {
            Self::Relation { from, to, .. } => {
                let mut subjects = vec![from.clone()];
                if !subjects.contains(to) {
                    subjects.push(to.clone());
                }
                subjects
            }
            Self::Evidence { subject, .. } => vec![subject.clone()],
            Self::Policy { .. } => Vec::new(),
        }
    }
}

/// Una relacion, con la forma que el diff necesita.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationFact {
    /// De que relacion se trata.
    pub relation: RelationType,
    /// El nodo del que parte.
    pub from: Hrn,
    /// El nodo al que llega.
    pub to: Hrn,
}

impl RelationFact {
    /// La relacion.
    pub fn new(relation: RelationType, from: Hrn, to: Hrn) -> Self {
        Self { relation, from, to }
    }

    /// La identidad de la relacion: la que dice si es la misma o no.
    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.relation.as_str(), self.from, self.to)
    }
}

impl From<&HypotheticalClaim> for RelationFact {
    fn from(claim: &HypotheticalClaim) -> Self {
        match claim {
            HypotheticalClaim::Relation {
                relation, from, to, ..
            } => RelationFact::new(relation.clone(), from.clone(), to.clone()),
            _ => unreachable!("una hipotetica que no es una relacion no tiene relacion"),
        }
    }
}
