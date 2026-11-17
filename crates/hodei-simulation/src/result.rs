//! El contrato versionado del resultado de una simulacion.
//!
//! El diff de la simulacion dice que relaciones aparecen, que relaciones
//! desaparecen y que decisiones cambian. Eso es el **interior** del
//! resultado. El resultado es el contrato de fuera: lo que un consumidor
//! --una consola, un informe, un segundo servicio-- recibe, y por eso
//! tiene que estar versionado, separar lo observado de lo hipotetico y
//! decir que ha cubierto y que no.
//!
//! # Las tres separaciones
//!
//! El contrato separa tres cosas que se confunden muy facil:
//!
//! - **Lo observado**: el estado real, tal como se leyo. Las relaciones
//!   que estaban en el grafo, la politica que estaba en vigor y la
//!   evidencia que se consumio para decidir.
//! - **Lo hipotetico**: los escenarios que se superponen. Van marcados
//!   como hipoteticos, y **no se publican como hechos del grafo**: una
//!   hipotetica que se confunde con un hecho es una mentira con formato
//!   de dato.
//! - **Lo que no se ha cubierto**: la cobertura y los conflictos. Un
//!   resultado que no dice lo que no ha mirado parece completo.
//!
//! # El contrato viaja sin objetos de politica
//!
//! La politica COMPLETA no viaja: es un objeto grande, con forma de
//! politica, y un contrato que lo lleva deja de ser portable. Lo que
//! viaja es el **sello** de la politica --su identificador y su
//! version--, que es lo que hace falta para saber que reglas se
//! aplicaron. Los helpers de comparacion y normalizacion trabajan con
//! sellos y jamas con objetos de politica.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::domain::read_set::{ReadKind, ReadSet};
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::base::SimulationBase;
use crate::compare;
use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::fork::SimulationFork;
use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};

/// La version del contrato.
///
/// El contrato cambia, y cuando cambia se declara con OTRO numero: un
/// consumidor que lee la version `1.0.0` sabe lo que puede esperar de
/// el, y un consumidor que lee otra version sabe que tiene que mirar.
pub const SIMULATION_RESULT_VERSION: &str = "1.0.0";

/// El sello de una politica: su identificador y su version.
///
/// Es lo que viaja en vez de la politica: lo que hace falta para saber
/// que reglas se aplicaron, sin arrastrar el objeto entero.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PolicyStamp {
    /// El identificador del conjunto de reglas.
    pub id: String,
    /// La version del conjunto de reglas.
    pub version: String,
}

impl PolicyStamp {
    /// El sello de una politica.
    pub fn of(policy: &TrustPolicy) -> Self {
        Self {
            id: policy.id.clone(),
            version: policy.version.clone(),
        }
    }
}

/// El sello de una evidencia: su identificador y el digest de su
/// contenido.
///
/// El digest es lo que permite comprobar el origen: si la evidencia
/// cambia de contenido, cambia el digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EvidenceStamp {
    /// El identificador de la evidencia.
    pub id: String,
    /// El digest de su contenido.
    pub digest: String,
}

/// La version de una fuente de lecturas.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceStamp {
    /// La fuente: el grafo, la evidencia, la politica.
    pub source: String,
    /// La version que se leyo de esa fuente.
    pub version: u64,
}

/// El sello de la base: sobre que mundo se calculo el resultado.
///
/// Es lo que hace auditable el resultado: sin el, una respuesta dice
/// que pasaria si y no dice sobre que version de la verdad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseStamp {
    /// La cuenta a la que pertenece la simulacion.
    pub account: String,
    /// El instante en el que se evaluo.
    pub evaluated_at: DateTime<Utc>,
    /// La generacion del grafo contra la que se evaluo.
    pub generation: Option<u64>,
    /// El sello del contenido leido: el digest de las relaciones leidas.
    pub seal: String,
    /// La huella del manifiesto de lecturas.
    pub fingerprint: String,
    /// La posicion del ledger que declara el punto de partida, si se
    /// sabe. Es una posicion DECLARADA, no verificada.
    pub declared_ledger_position: Option<u64>,
    /// La version de cada fuente que se leyo.
    pub sources: Vec<SourceStamp>,
}

/// Lo OBSERVADO: el estado real, tal como se leyo.
///
/// Lo que esta aqui estuvo en el mundo. Lo que no esta aqui, no estuvo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedState {
    /// Las relaciones observadas, en orden canonico y sin duplicados.
    pub relations: Vec<RelationFact>,
    /// La politica que estaba en vigor.
    pub policy: PolicyStamp,
    /// La evidencia que se consumio para decidir.
    pub evidence: Vec<EvidenceStamp>,
}

/// Un escenario hipotetico, en forma portable.
///
/// Es una hipotetica y se dice: viaja en su propio campo y nunca se
/// mezcla con las relaciones observadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hypothesis {
    /// La simulacion a la que pertenece la hipotetica.
    pub simulation: String,
    /// El momento en que se creo la hipotetica.
    pub created_at: DateTime<Utc>,
    /// Lo que dice la hipotetica.
    pub kind: HypothesisKind,
    /// Los sujetos que la hipotetica toca.
    pub subjects: Vec<Hrn>,
}

/// Lo que dice una hipotetica.
///
/// Y en ninguna de las variantes viaja la politica completa: la
/// hipotetica de politica lleva su sello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    /// Una relacion que existiria.
    Relation {
        /// De que relacion se trata.
        relation: RelationType,
        /// El nodo del que parte.
        from: Hrn,
        /// El nodo al que llega.
        to: Hrn,
        /// La identidad de la relacion que SUSTITUYE, si sustituye a
        /// alguna.
        replaces: Option<String>,
    },
    /// Un riesgo que habria.
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
    UntrustedWindow {
        /// El principio de la ventana.
        from: DateTime<Utc>,
        /// El final de la ventana.
        to: DateTime<Utc>,
        /// Los sujetos a los que alcanza la ventana.
        subjects: Vec<Hrn>,
    },
    /// Una politica que estaria en vigor.
    Policy {
        /// El sello de la politica, no la politica.
        policy: PolicyStamp,
    },
}

impl Hypothesis {
    /// La clave canonica: dos hipoteticas con la misma clave son la misma
    /// hipotetica.
    ///
    /// La clave es lo que ordena el resultado, y por eso no depende del
    /// orden en que se declararon los hechos.
    pub fn the_key(&self) -> String {
        match &self.kind {
            HypothesisKind::Relation {
                relation,
                from,
                to,
                replaces,
            } => format!(
                "relation|{}|{}|{}|{}",
                relation.as_str(),
                from,
                to,
                replaces.clone().unwrap_or_default()
            ),
            HypothesisKind::Risk {
                subject,
                advisory,
                critical,
                reachable,
            } => format!("risk|{subject}|{advisory}|{critical}|{reachable}"),
            HypothesisKind::UntrustedWindow { from, to, subjects } => format!(
                "window|{}|{}|{}",
                from.to_rfc3339(),
                to.to_rfc3339(),
                subjects
                    .iter()
                    .map(|subject| subject.to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            ),
            HypothesisKind::Policy { policy } => format!("policy|{}|{}", policy.id, policy.version),
        }
    }
}

/// La cobertura: que se ha mirado y que no.
///
/// Un resultado que no dice lo que no ha visto parece completo, y un
/// resultado que parece completo se usa como si lo fuera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Los sujetos DECLARADOS: los que alguien nombro.
    pub declared_subjects: Vec<Hrn>,
    /// Los sujetos EVALUADOS: los declarados y los que traen los hechos.
    pub evaluated_subjects: Vec<Hrn>,
    /// Las relaciones que se leyeron del grafo real.
    pub relations_read: usize,
    /// Las relaciones de la cuenta que NO se leyeron.
    ///
    /// La simulacion lee las relaciones de los sujetos que toca, no las
    /// de toda la cuenta. El numero dice lo que no se ha visto.
    pub relations_not_read: usize,
    /// Lo que se quedo fuera, con su motivo.
    pub omitted: Vec<Omission>,
}

/// Lo que se quedo fuera de la cobertura.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Omission {
    /// El sujeto que se quedo fuera, si se sabe cual es.
    pub subject: Option<Hrn>,
    /// El motivo.
    pub reason: String,
}

/// Un conflicto que la simulacion declara.
///
/// Un conflicto no es un fallo: es una contradiccion del escenario que se
/// dice en vez de esconderse. Un overlay que cierra un ciclo y se
/// presenta como un overlay normal es un overlay que nadie puede leer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// La clase del conflicto.
    pub kind: ConflictKind,
    /// El detalle, en texto.
    pub detail: String,
    /// Los sujetos que el conflicto toca.
    pub subjects: Vec<Hrn>,
}

/// Las clases de conflicto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// El overlay cierra un ciclo.
    Cycle,
    /// La misma relacion se dice dos veces.
    Duplicate,
    /// Dos hipoteticas se contradicen.
    Contradiction,
}

/// El contrato versionado del resultado de una simulacion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationResult {
    /// La version del contrato.
    pub version: String,
    /// La simulacion a la que pertenece el resultado.
    pub simulation: String,
    /// La cuenta de la simulacion.
    pub account: String,
    /// El sello de la base: sobre que mundo se calculo.
    pub base: BaseStamp,
    /// Lo que se observo.
    pub observed: ObservedState,
    /// Los escenarios hipoteticos, en orden canonico.
    pub hypotheses: Vec<Hypothesis>,
    /// El diff de las relaciones.
    pub structural: StructuralDiff,
    /// Las decisiones que cambian, en orden canonico.
    pub decisions: Vec<DecisionChange>,
    /// La cobertura.
    pub coverage: Coverage,
    /// Los conflictos que el escenario declara.
    pub conflicts: Vec<Conflict>,
}

impl SimulationResult {
    /// El resultado a partir del diff, del fork y de lo que se observo.
    ///
    /// La construccion es **pura**: no lee, no escribe y no decide. Lo
    /// que se le da es lo que ya se sabe, y lo que sale es el contrato.
    ///
    /// El numero de relaciones de la cuenta es el total del grafo, no el
    /// de los sujetos tocados: la cobertura dice lo que no se ha leido, y
    /// para eso hace falta el total.
    pub fn of(
        diff: &SimulationDiff,
        fork: &SimulationFork,
        observed: &[RelationFact],
        relations_in_the_account: usize,
    ) -> Self {
        let observed = compare::normalize_relations(observed);
        let overlay = fork.relations_with(&observed);
        Self {
            version: SIMULATION_RESULT_VERSION.to_string(),
            simulation: fork.id.clone(),
            account: diff.base.account.clone(),
            base: the_base_stamp_of(&diff.base),
            observed: ObservedState {
                relations: observed.clone(),
                policy: PolicyStamp::of(&fork.base_policy),
                evidence: the_evidence_of(&diff.base.read_set),
            },
            hypotheses: the_hypotheses_of(fork),
            structural: diff.structural.clone(),
            decisions: compare::normalize_decisions(&diff.decisions),
            coverage: the_coverage_of(fork, &observed, relations_in_the_account),
            conflicts: the_conflicts_of(fork, &observed, &overlay),
        }
    }

    /// Si el resultado no dice nada.
    ///
    /// Un resultado sin diferencias, sin decisiones y sin conflictos no
    /// dice nada: y un resultado que no dice nada no es un resultado.
    pub fn is_empty(&self) -> bool {
        self.structural.is_empty()
            && self.decisions.is_empty()
            && self.conflicts.is_empty()
            && self.hypotheses.is_empty()
    }
}

/// El sello de la base de un diff.
fn the_base_stamp_of(base: &SimulationBase) -> BaseStamp {
    let mut sources: Vec<SourceStamp> = base
        .read_set
        .generations
        .iter()
        .map(|(source, version)| SourceStamp {
            source: source.clone(),
            version: *version,
        })
        .collect();
    sources.sort();
    BaseStamp {
        account: base.account.clone(),
        evaluated_at: base.evaluated_at,
        generation: base
            .read_set
            .generations
            .iter()
            .find(|(source, _)| source == "graph")
            .map(|(_, version)| *version),
        seal: base.seal.clone(),
        fingerprint: base.fingerprint(),
        declared_ledger_position: base.declared_ledger_position,
        sources,
    }
}

/// La evidencia que se leyo para decidir, en orden canonico.
fn the_evidence_of(read_set: &ReadSet) -> Vec<EvidenceStamp> {
    let mut evidence: Vec<EvidenceStamp> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .map(|stamp| EvidenceStamp {
            id: stamp.resource.clone(),
            digest: stamp.verified_by.clone(),
        })
        .collect();
    evidence.sort();
    evidence.dedup();
    evidence
}

/// Los escenarios hipoteticos del fork, en orden canonico.
///
/// La politica hipotetica del fork es una hipotetica mas: no viaja en
/// la lista de hechos --porque es un campo del fork y no un hecho-- pero
/// es tan hipotetica como el resto, y si no se declara el consumidor no
/// sabe que reglas se aplicaron.
fn the_hypotheses_of(fork: &SimulationFork) -> Vec<Hypothesis> {
    let mut hypotheses: Vec<Hypothesis> = fork.facts.iter().map(the_hypothesis_of).collect();
    if let Some(policy) = &fork.policy {
        hypotheses.push(Hypothesis {
            simulation: fork.id.clone(),
            created_at: fork.created_at,
            kind: HypothesisKind::Policy {
                policy: PolicyStamp::of(policy),
            },
            subjects: Vec::new(),
        });
    }
    hypotheses.sort_by_key(|hypothesis| hypothesis.the_key());
    hypotheses
}

/// Un hecho hipotetico, proyectado a forma portable.
fn the_hypothesis_of(fact: &Hypothetical) -> Hypothesis {
    let kind = match &fact.claim {
        HypotheticalClaim::Relation {
            relation,
            from,
            to,
            replaces,
        } => HypothesisKind::Relation {
            relation: relation.clone(),
            from: from.clone(),
            to: to.clone(),
            replaces: replaces.as_ref().map(RelationFact::identity),
        },
        HypotheticalClaim::Risk {
            subject,
            advisory,
            critical,
            reachable,
        } => HypothesisKind::Risk {
            subject: subject.clone(),
            advisory: advisory.clone(),
            critical: *critical,
            reachable: *reachable,
        },
        HypotheticalClaim::UntrustedWindow { from, to, subjects } => {
            HypothesisKind::UntrustedWindow {
                from: *from,
                to: *to,
                subjects: subjects.clone(),
            }
        }
        HypotheticalClaim::Policy { policy } => HypothesisKind::Policy {
            policy: PolicyStamp::of(policy),
        },
    };
    let mut subjects = fact.claim.subjects();
    subjects.sort_by_key(|subject| subject.to_string());
    subjects.dedup();
    Hypothesis {
        simulation: fact.tag.simulation.clone(),
        created_at: fact.tag.created_at,
        kind,
        subjects,
    }
}

/// La cobertura de la simulacion.
fn the_coverage_of(
    fork: &SimulationFork,
    observed: &[RelationFact],
    relations_in_the_account: usize,
) -> Coverage {
    let mut declared = fork.subjects.clone();
    declared.sort_by_key(|subject| subject.to_string());
    declared.dedup();
    let mut evaluated = fork.affected_subjects();
    evaluated.sort_by_key(|subject| subject.to_string());
    evaluated.dedup();

    let relations_read = observed.len();
    let relations_not_read = relations_in_the_account.saturating_sub(relations_read);
    let mut omitted: Vec<Omission> = Vec::new();
    if relations_not_read > 0 {
        omitted.push(Omission {
            subject: None,
            reason: format!(
                "the simulation reads the relations of the subjects it touches: {relations_not_read} \
                 relations of the account were not read"
            ),
        });
    }

    Coverage {
        declared_subjects: declared,
        evaluated_subjects: evaluated,
        relations_read,
        relations_not_read,
        omitted,
    }
}

/// Los conflictos que el escenario declara.
///
/// Los tres son del overlay y ninguno es del mundo real: un ciclo que ya
/// estaba no es de la simulacion, una relacion que ya existia no la
/// introduce la hipotetica y una contradiccion entre dos hipoteticas es
/// de quien las declaro.
fn the_conflicts_of(
    fork: &SimulationFork,
    observed: &[RelationFact],
    overlay: &[RelationFact],
) -> Vec<Conflict> {
    let added = fork.added_relations();
    let removed = fork.removed_relations();
    let hypothetical: Vec<String> = added.iter().map(RelationFact::identity).collect();

    let mut conflicts: Vec<Conflict> = Vec::new();
    for cycle in compare::the_cycles_introduced_by(overlay, &hypothetical) {
        conflicts.push(Conflict {
            kind: ConflictKind::Cycle,
            detail: format!(
                "the overlay closes a cycle: {}",
                cycle.relations.join(" -> ")
            ),
            subjects: cycle.subjects,
        });
    }
    for identity in compare::the_duplicates_of(&added, observed) {
        conflicts.push(Conflict {
            kind: ConflictKind::Duplicate,
            detail: format!("the relation {identity} is said twice"),
            subjects: compare::the_subjects_of(&identity, observed),
        });
    }
    for identity in compare::the_contradictions_of(&added, &removed) {
        conflicts.push(Conflict {
            kind: ConflictKind::Contradiction,
            detail: format!("the relation {identity} is added and replaced at the same time"),
            subjects: compare::the_subjects_of(&identity, observed),
        });
    }
    conflicts
}
