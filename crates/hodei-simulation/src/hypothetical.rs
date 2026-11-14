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
    /// Un riesgo que habria, con lo que se sabe de el.
    ///
    /// El riesgo es un hecho del sujeto: una vulnerabilidad que le
    /// llega. Y lo que decide la politica no es el identificador de la
    /// vulnerabilidad, sino si es critica y si es alcanzable.
    Risk {
        /// El sujeto al que llega.
        subject: Hrn,
        /// La vulnerabilidad.
        advisory: String,
        /// Si es critica.
        critical: bool,
        /// Si es alcanzable.
        reachable: bool,
    },
    /// Una ventana en la que un constructor no se fia.
    ///
    /// Todo lo que se observo dentro de la ventana deja de estar
    /// verificado: es lo que pasa cuando un constructor esta comprometido
    /// durante un rato, y las piezas de evidencia de ese rato son
    /// precisamente las que no valen.
    ///
    /// La ventana **nombra a los sujetos** a los que alcanza, porque sin
    /// ellos la simulacion no sabria a quien mirar: una ventana es un
    /// rango de tiempo, y un rango de tiempo no dice de quien es la
    /// culpa.
    UntrustedWindow {
        /// El principio de la ventana.
        from: DateTime<Utc>,
        /// El final de la ventana.
        to: DateTime<Utc>,
        /// Los sujetos a los que alcanza la ventana.
        subjects: Vec<Hrn>,
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
            Self::Risk { subject, .. } => vec![subject.clone()],
            Self::UntrustedWindow { subjects, .. } => subjects.clone(),
            Self::Policy { .. } => Vec::new(),
        }
    }
}

/// Una relacion, con la forma que el diff necesita.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationFact {
    /// De que relacion se trata.
    pub relation: RelationType,
    /// El nodo del que parte.
    pub from: Hrn,
    /// El nodo al que llega.
    pub to: Hrn,
    /// La evidencia que la sostiene, si se conoce.
    ///
    /// No es parte de la identidad de la relacion: es el ORIGEN, y lo
    /// que lo verifica. Una relacion a la que se le anade un apoyo sigue
    /// siendo la misma relacion, y por eso el diff estructural habla de
    /// relaciones y no de apoyos. El cambio de apoyo se ve en el sello
    /// de la base, que si compara la evidencia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl PartialEq for RelationFact {
    /// Dos relaciones son la MISMA relacion cuando tienen la misma
    /// identidad: el tipo y los dos extremos.
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for RelationFact {}

impl RelationFact {
    /// La relacion.
    pub fn new(relation: RelationType, from: Hrn, to: Hrn) -> Self {
        Self {
            relation,
            from,
            to,
            evidence: None,
        }
    }

    /// La relacion, con la evidencia que la sostiene.
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
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
