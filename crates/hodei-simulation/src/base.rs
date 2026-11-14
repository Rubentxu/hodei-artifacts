//! La base fijada: sobre que mundo se evalua la simulacion.
//!
//! Una simulacion que no dice sobre que version de la verdad se
//! calculo es una respuesta sin apellido: el mismo "que pasaria si"
//! puede ser verdad o falso segun lo que haya cambiado entre la
//! pregunta y la respuesta. Y una simulacion que se apoya en un mundo
//! que ya no existe no es una simulacion vieja: es una simulacion
//! **equivocada**, porque responde por un estado que nadie tiene.
//!
//! # Las dos partes del sello
//!
//! La base se sella por partida doble, y las dos hacen falta:
//!
//! - La **generacion** del grafo dice si la HISTORIA del grafo ha
//!   avanzado. Es un contador monotono, barato de leer, y es lo que
//!   usa el investigador para lo mismo.
//! - El **sello de contenido** dice si lo que se LIO se ha movido. La
//!   generacion no basta: el grafo se escribe tambien por caminos que
//!   no pasan por su historia de deltas, y una relacion sustituida por
//!   otra mantiene el numero de relaciones. Solo el contenido
//!   distingue una cosa de la otra.
//!
//! Y el sello de contenido se compara por su digest, no por el numero:
//! el numero de relaciones no es una version, es un recuento, y un
//! recuento no dice que las relaciones sean las mismas.
//!
//! # El reloj
//!
//! La base lleva el instante en el que se evaluo. El reloj no es del
//! sistema: lo recibe quien simula, y por eso dos simulaciones con la
//! misma base y el mismo reloj dan el mismo resultado. Un nucleo de
//! decision que lee el reloj de pared no es reproducible, y una
//! simulacion que no es reproducible no se puede auditar.
//!
//! # El alcance de la generacion
//!
//! La generacion es **de la cuenta**, no del sujeto: es el contador de
//! la historia del grafo entero. Una escritura que no toca el conjunto
//! que la simulacion lee tambien la invalida. Es a proposito --fallar
//! cerrado antes que devolver una respuesta sobre un mundo que se
//! movió--, pero conviene saberlo: la simulacion es mas estricta de lo
//! que parece.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::read_set::{ReadKind, ReadSet, ReadStamp};
use hodei_trust_policy::TrustPolicy;
use serde::{Deserialize, Serialize};

use crate::hypothetical::RelationFact;

/// La base sobre la que se evalua una simulacion.
///
/// Es lo que se fija ANTES de simular y lo que se comprueba DESPUES:
/// si el mundo se ha movido, la simulacion se dice obsoleta en lugar de
/// devolver un diff que nadie sabe sobre que mundo vale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SimulationBase {
    /// La cuenta a la que pertenece la base.
    pub account: String,
    /// El instante en el que se evaluo.
    pub evaluated_at: DateTime<Utc>,
    /// El manifiesto de lo que se leyo: las generaciones de las fuentes
    /// y los sellos de cada recurso.
    pub read_set: ReadSet,
    /// El sello del contenido leido: el digest de las relaciones leidas
    /// con su evidencia.
    pub seal: String,
    /// La posicion del ledger que declara el punto de partida, si se
    /// sabe.
    ///
    /// Va APARTE de las generaciones del manifiesto a proposito: las
    /// generaciones son las versiones que se han VERIFICADO contra su
    /// fuente, y esta posicion no se puede verificar aqui --la
    /// simulacion no tiene un puerto de ledger contra el que
    /// comprobarla--. Declararla como generacion la haria pasar por
    /// comprobada.
    #[serde(default)]
    pub declared_ledger_position: Option<u64>,
}

impl SimulationBase {
    /// Una base vacia de una cuenta.
    pub fn new(account: impl Into<String>, evaluated_at: DateTime<Utc>) -> Self {
        Self {
            account: account.into(),
            evaluated_at,
            read_set: ReadSet::empty(),
            seal: String::new(),
            declared_ledger_position: None,
        }
    }

    /// Registra la generacion de una fuente.
    pub fn with_generation(mut self, source: impl Into<String>, version: u64) -> Self {
        self.read_set.generation(source, version);
        self
    }

    /// Registra las relaciones leidas: sus sellos y el sello de su
    /// contenido.
    ///
    /// Las dos cosas van juntas a proposito: una base que sellara las
    /// relaciones pero no su contenido no podria comprobar si lo leido
    /// sigue siendo lo mismo, que es justo lo que el sello de contenido
    /// comprueba.
    pub fn with_relations(mut self, relations: &[RelationFact]) -> Self {
        for relation in relations {
            self.read_set.read(ReadStamp::relation(
                relation.identity(),
                relation.evidence.clone().unwrap_or_default(),
            ));
        }
        // El sello se calcula sobre el MANIFIESTO y no sobre una lista
        // aparte: asi cubre todo lo que la base ha leido, aunque se lea
        // en varias tandas, y no puede quedar desincronizado de los
        // sellos que lo nombran.
        self.seal = the_seal_of_the_read_set(&self.read_set);
        self
    }

    /// Registra la politica contra la que se decide.
    pub fn with_policy(mut self, policy: &TrustPolicy) -> Self {
        self.read_set
            .read(ReadStamp::policy(policy.id.clone(), policy.version.clone()));
        self
    }

    /// Registra lo que se ha leido para DECIDIR.
    ///
    /// Es la parte de la base que no es del grafo: la evidencia que el
    /// nucleo de decision consume. Sin ella la base diria sobre que
    /// mundo se calculo sin decir que evidencia lo decidio, y un diff
    /// al que le entra evidencia nueva se atribuiria al hecho
    /// hipotetico cuando la causa fue otra.
    pub fn with_reads(mut self, reads: &ReadSet) -> Self {
        for stamp in &reads.stamps {
            self.read_set.read(stamp.clone());
        }
        self
    }

    /// La huella del manifiesto de lecturas: el digest de lo que se leyo,
    /// sin releerlo entero.
    pub fn fingerprint(&self) -> String {
        self.read_set.fingerprint()
    }

    /// Si la GENERACION del mundo ya no es la que se leyo.
    ///
    /// La comparacion es contra la generacion del GRAFO, la unica fuente
    /// que se puede comprobar aqui. Y una base que no ha registrado
    /// generacion no dice que el mundo se movio: no dice nada, que no es
    /// lo mismo.
    pub fn is_the_generation_stale(&self, generation: u64) -> bool {
        self.read_set
            .generations
            .iter()
            .find(|(source, _)| source == "graph")
            .map(|(_, expected)| *expected != generation)
            .unwrap_or(false)
    }

    /// Si el CONTENIDO leido ya no es el que se leyo.
    ///
    /// El sello compara el digest de lo leido con el digest de lo que se
    /// acaba de leer, y no el numero de relaciones: un numero no es una
    /// version.
    pub fn is_the_content_stale(&self, seal: &str) -> bool {
        self.seal != seal
    }

    /// Si la EVIDENCIA que se leyo para decidir ya no es la misma.
    ///
    /// La evidencia no toca el grafo: entra en el almacen de evidencia y
    /// no cambia ni el numero de relaciones ni la generacion. Por eso
    /// necesita su propia comprobacion, o una evidencia que entra
    /// mientras se evalua pasaria desapercibida -- y el cambio de
    /// decision se atribuiria al hecho hipotetico cuando la causa fue la
    /// evidencia que entro.
    pub fn is_the_evidence_stale(&self, reads: &ReadSet) -> bool {
        the_evidence_of(&self.read_set) != the_evidence_of(reads)
    }
}

/// Los sellos de la evidencia de un manifiesto, en orden canonico.
///
/// Se comparan como CONJUNTOS: la evidencia que se leyo y la que se
/// acaba de leer son las mismas si sus sellos son los mismos, sin
/// importar el orden en que se leyeron.
fn the_evidence_of(read_set: &ReadSet) -> Vec<ReadStamp> {
    let mut stamps: Vec<ReadStamp> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .cloned()
        .collect();
    stamps.sort();
    stamps
}

/// La huella de la evidencia leida: el digest de sus sellos.
///
/// Es lo que dice un `StaleBase` de evidencia: que evidencia se leyo y
/// que evidencia se ha encontrado, sin volcar los contenidos.
pub fn the_evidence_fingerprint(read_set: &ReadSet) -> String {
    ContentDigest::of_bytes(format!("{:?}", the_evidence_of(read_set)).as_bytes()).to_string()
}

/// El sello del contenido leido: el digest de las relaciones, con su
/// evidencia.
///
/// El digest se calcula sobre la lista ORDENADA de las relaciones, y no
/// sobre el orden en que las dio el grafo: dos lecturas del mismo
/// grafo dan el mismo sello, se lean en el orden en que se lean.
pub fn the_seal_of(relations: &[RelationFact]) -> String {
    let mut read_set = ReadSet::empty();
    for relation in relations {
        read_set.read(ReadStamp::relation(
            relation.identity(),
            relation.evidence.clone().unwrap_or_default(),
        ));
    }
    the_seal_of_the_read_set(&read_set)
}

/// El sello de las relaciones de un manifiesto.
///
/// Se sellan SOLO las relaciones: la politica y las generaciones son
/// otras fuentes, con su propia verificacion, y meterlas en el mismo
/// digest haria que un cambio de politica pareciese un cambio de
/// contenido.
///
/// Cada linea lleva la LONGITUD de sus dos partes. Sin longitudes, dos
///lecturas distintas podrian dar el mismo texto --una relacion cuya
/// evidencia lleva un salto de linea dentro-- y el digest de dos
/// mundos distintos seria el mismo. Un sello que no distingue no
/// comprueba nada.
fn the_seal_of_the_read_set(read_set: &ReadSet) -> String {
    let mut lines: Vec<String> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Relation)
        .map(|stamp| {
            format!(
                "{}:{}:{}:{}",
                stamp.resource.len(),
                stamp.resource,
                stamp.verified_by.len(),
                stamp.verified_by
            )
        })
        .collect();
    lines.sort();
    ContentDigest::of_bytes(lines.join("\n").as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::domain::graph::RelationType;
    use kernel::Hrn;

    /// El sujeto de la prueba.
    fn subject() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
    }

    /// El componente del que depende.
    fn component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
    }

    /// El sello no depende del ORDEN en que se leen las relaciones: dos
    /// lecturas del mismo grafo dan el mismo sello.
    #[test]
    fn the_seal_does_not_depend_on_the_order_of_the_reads() {
        let first = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let second = RelationFact::new(RelationType::Contains, component(), subject())
            .with_evidence("evd-2");

        let one = the_seal_of(&[first.clone(), second.clone()]);
        let other = the_seal_of(&[second, first]);
        assert_eq!(one, other);
    }

    /// Y el sello SI cambia cuando lo leido cambia: una relacion
    /// sustituida por otra deja un sello distinto, aunque el numero de
    /// relaciones siga siendo el mismo.
    #[test]
    fn the_seal_changes_when_a_relation_is_replaced() {
        let before = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let after = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-2");

        assert_ne!(
            the_seal_of(&[before]),
            the_seal_of(&[after]),
            "el mismo numero de relaciones, distinto sello"
        );
    }

    /// Y tambien cuando lo que cambia es la relacion misma.
    #[test]
    fn the_seal_changes_when_the_relation_changes() {
        let other = Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2")
            .expect("un HRN");
        let before = RelationFact::new(RelationType::DependsOn, subject(), component());
        let after = RelationFact::new(RelationType::DependsOn, subject(), other);

        assert_ne!(the_seal_of(&[before]), the_seal_of(&[after]));
    }

    /// Dos lecturas DISTINTAS no pueden dar el mismo sello: el sello
    /// codifica con longitudes, y sin longitudes una evidencia con un
    /// salto de linea dentro podria disfrazarse de dos relaciones.
    #[test]
    fn the_seal_tells_two_different_reads_apart() {
        let relation = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");

        // Una relacion con la evidencia "x\n<otra>|y" y dos relaciones
        // con "x" e "y": sin longitudes, el texto es el mismo.
        let disguised = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence(format!("x\n{}|y", relation.identity()));

        let one = the_seal_of(&[relation.clone(), disguised.clone()]);
        let other = the_seal_of(std::slice::from_ref(&relation));
        assert_ne!(
            one, other,
            "una lectura con una evidencia disfrazada no es la misma lectura"
        );
    }

    /// La base esta obsoleta cuando la generacion se movio o cuando el
    /// contenido leido cambio.
    #[test]
    fn a_base_is_stale_when_the_generation_or_the_content_moved() {
        let relation = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let seal = the_seal_of(std::slice::from_ref(&relation));
        let base = SimulationBase::new("acme", Utc::now())
            .with_generation("graph", 7)
            .with_relations(std::slice::from_ref(&relation));

        assert!(
            !base.is_the_generation_stale(7),
            "la misma generacion no la vuelve obsoleta"
        );
        assert!(
            base.is_the_generation_stale(8),
            "una generacion nueva la vuelve obsoleta"
        );
        assert!(
            !base.is_the_content_stale(&seal),
            "el mismo contenido no la vuelve obsoleta"
        );
        assert!(
            base.is_the_content_stale("otro-sello"),
            "un contenido nuevo la vuelve obsoleta"
        );
    }

    /// El sello cubre TODO lo que la base ha leido, aunque se lea en
    /// varias tandas: una base que sellara solo la ultima tanda dejaria
    /// sin vigilar lo leido antes, y el sello mentiria sobre su alcance.
    #[test]
    fn the_seal_covers_everything_the_base_read() {
        let first = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let second = RelationFact::new(RelationType::Contains, component(), subject())
            .with_evidence("evd-2");

        let base = SimulationBase::new("acme", Utc::now())
            .with_relations(std::slice::from_ref(&first))
            .with_relations(std::slice::from_ref(&second));

        assert_eq!(
            base.seal,
            the_seal_of(&[first.clone(), second.clone()]),
            "el sello cubre las dos tandas"
        );
        assert!(
            base.is_the_content_stale(&the_seal_of(std::slice::from_ref(&first))),
            "y si se lee solo la primera, el contenido se movio"
        );
    }

    /// La base guarda lo que se leyo para DECIDIR: la evidencia que
    /// consume el nucleo de decision es parte de la base, porque sin
    /// ella la base no dice sobre que mundo se calculo la respuesta.
    #[test]
    fn the_base_keeps_what_the_decision_read() {
        let mut reads = ReadSet::empty();
        reads.read(ReadStamp::evidence("evd-1", "sha256:aaa"));

        let base = SimulationBase::new("acme", Utc::now()).with_reads(&reads);

        assert!(
            base.read_set
                .stamps
                .iter()
                .any(|stamp| stamp.kind == ReadKind::Evidence),
            "la evidencia leida esta en el manifiesto de la base"
        );
    }
}
