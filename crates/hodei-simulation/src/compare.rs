//! La comparacion pura: lo que se ordena, lo que se deduplica y lo que
//! se contradice.
//!
//! Los helpers de aqui son **puros**: reciben relaciones, identidades y
//! decisiones, y devuelven relaciones, identidades y decisiones. No leen,
//! no escriben, no deciden y no llevan objetos de politica: la politica
//! es un dato de la simulacion, y un helper que la llevara seria un
//! helper que no se podria comparar con otro.
//!
//! Y son funciones y no metodos porque su forma no depende de quien los
//! llama: el mismo helper ordena el overlay de un fork y el de otro, y
//! los dos resultados se comparan.

use std::collections::{BTreeMap, BTreeSet};

use kernel::Hrn;

use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::hypothetical::RelationFact;

/// Un ciclo: las relaciones que lo cierran y los sujetos que toca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    /// Las relaciones del ciclo, en orden canonico.
    pub relations: Vec<String>,
    /// Los sujetos del ciclo, en orden canonico.
    pub subjects: Vec<Hrn>,
}

/// Las relaciones normalizadas: sin duplicados y en orden canonico.
///
/// La deduplicacion es por IDENTIDAD --el tipo y los dos extremos--, y de
/// dos relaciones con la misma identidad gana la que lleva la evidencia
/// MAS PEQUENA: el resultado no puede depender del orden en que el
/// almacen devolvio las filas, porque un resultado que depende del orden
/// de las filas no es un resultado del mundo.
pub fn normalize_relations(relations: &[RelationFact]) -> Vec<RelationFact> {
    let mut chosen: BTreeMap<String, RelationFact> = BTreeMap::new();
    for relation in relations {
        let identity = relation.identity();
        let keep = match chosen.get(&identity) {
            Some(known) => the_evidence_of(relation) < the_evidence_of(known),
            None => true,
        };
        if keep {
            chosen.insert(identity, relation.clone());
        }
    }
    chosen.into_values().collect()
}

/// Las decisiones en orden canonico: por sujeto.
///
/// El diff de decisiones se compara con otro diff de decisiones, y dos
/// listas que se leen en orden distinto no son la misma lista.
pub fn normalize_decisions(decisions: &[DecisionChange]) -> Vec<DecisionChange> {
    let mut decisions = decisions.to_vec();
    decisions.sort_by_key(|change| change.subject.to_string());
    decisions
}

/// Los ciclos del overlay: las vueltas que se cierran.
///
/// Un ciclo es un conjunto de relaciones que se pisan entre si: A llega a
/// B y B vuelve a A.
///
/// Y son TODOS los ciclos, sin filtrar: el filtro de los que introduce
/// la hipotetica es otra funcion, porque son dos preguntas distintas y
/// una sola funcion que las mezcla no se puede probar.
pub fn the_cycles_of(overlay: &[RelationFact]) -> Vec<Cycle> {
    let mut edges: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for relation in overlay {
        edges
            .entry(relation.from.to_string())
            .or_default()
            .push((relation.to.to_string(), relation.identity()));
    }
    // El recorrido tiene que ser el mismo con cualquier orden de entrada:
    // un ciclo que aparece en una ejecucion y no en otra no es un ciclo
    // del overlay, es un ciclo del recorrido.
    for list in edges.values_mut() {
        list.sort();
    }

    let mut cycles: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut on_the_path: BTreeSet<String> = BTreeSet::new();
    let mut path: Vec<(String, String)> = Vec::new();

    for start in edges.keys() {
        if visited.contains(start) {
            continue;
        }
        visit(
            start,
            &edges,
            &mut visited,
            &mut on_the_path,
            &mut path,
            &mut cycles,
        );
    }

    let mut found: Vec<Cycle> = Vec::new();
    for cycle in cycles {
        found.push(Cycle {
            subjects: the_subjects_of_all(&cycle, overlay),
            relations: cycle,
        });
    }
    found
}

/// Los ciclos que la HIPOTETICA introduce.
///
/// Un ciclo que ya estaba en el mundo real no lo introduce la
/// simulacion, y declararlo seria echarle la culpa a la hipotetica. Lo
/// que la simulacion introduce es lo que declara.
pub fn the_cycles_introduced_by(overlay: &[RelationFact], hypothetical: &[String]) -> Vec<Cycle> {
    the_cycles_of(overlay)
        .into_iter()
        .filter(|cycle| {
            cycle
                .relations
                .iter()
                .any(|identity| hypothetical.contains(identity))
        })
        .collect()
}

/// Los duplicados: las identidades que se dicen mas de una vez.
///
/// Una hipotesis que repite una relacion que ya esta en el mundo dice lo
/// mismo dos veces, y una hipotesis que repite otra hipotesis tambien.
/// Las dos se declaran: un diff que las guarda sin decir nada parece un
/// diff que no ha mirado.
pub fn the_duplicates_of(hypothetical: &[RelationFact], observed: &[RelationFact]) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut duplicates: BTreeSet<String> = BTreeSet::new();
    for relation in hypothetical {
        let identity = relation.identity();
        if !seen.insert(identity.clone()) {
            duplicates.insert(identity);
        }
    }
    for relation in hypothetical {
        let identity = relation.identity();
        if observed.iter().any(|one| one.identity() == identity) {
            duplicates.insert(identity);
        }
    }
    duplicates.into_iter().collect()
}

/// Las contradicciones: lo que una hipotesis dice y otra desmiente.
///
/// Una hipotesis que anade una relacion y otra que la sustituye se
/// contradicen: una dice que la relacion existe y la otra dice que no.
/// El overlay no puede obeyed las dos, y el resultado lo dice en vez de
/// elegir una por su cuenta.
pub fn the_contradictions_of(added: &[RelationFact], removed: &[RelationFact]) -> Vec<String> {
    let mut contradictions: Vec<String> = added
        .iter()
        .map(RelationFact::identity)
        .filter(|identity| removed.iter().any(|one| one.identity() == *identity))
        .collect();
    contradictions.sort();
    contradictions.dedup();
    contradictions
}

/// Los sujetos de una relacion, por su identidad.
pub fn the_subjects_of(identity: &str, relations: &[RelationFact]) -> Vec<Hrn> {
    the_subjects_of_all(std::slice::from_ref(&identity.to_string()), relations)
}

/// Los sujetos de un conjunto de relaciones, por sus identidades.
pub fn the_subjects_of_all(identities: &[String], relations: &[RelationFact]) -> Vec<Hrn> {
    let mut subjects: Vec<Hrn> = Vec::new();
    for relation in relations
        .iter()
        .filter(|one| identities.contains(&one.identity()))
    {
        for subject in [&relation.from, &relation.to] {
            if !subjects.contains(subject) {
                subjects.push(subject.clone());
            }
        }
    }
    subjects.sort_by_key(|subject| subject.to_string());
    subjects
}

/// La forma canonica de un diff: lo que dos simulaciones del mismo mundo
/// tienen que compartir.
///
/// El diff trae el manifiesto de lecturas, y ese manifiesto lleva el ORDEN
/// en que se leyo. Dos simulaciones del mismo mundo leen en orden distinto
/// y por eso sus manifiestos son iguales pero no identicos. La forma
/// canonica es la que se compara: el manifiesto ya ordenado y su huella.
///
/// El diff estructural y las decisiones ya salen en orden canonico, y
/// por eso se copian tal cual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalDiff {
    /// El diff de las relaciones.
    pub structural: StructuralDiff,
    /// Las decisiones que cambian, en orden canonico.
    pub decisions: Vec<DecisionChange>,
    /// El manifiesto de lecturas, ordenado.
    pub manifest: String,
    /// La huella del manifiesto.
    pub fingerprint: String,
    /// El sello del contenido leido.
    pub seal: String,
}

/// La forma canonica de un diff.
pub fn canonical(diff: &SimulationDiff) -> CanonicalDiff {
    CanonicalDiff {
        structural: diff.structural.clone(),
        decisions: normalize_decisions(&diff.decisions),
        manifest: diff.base.read_set.manifest(),
        fingerprint: diff.base.fingerprint(),
        seal: diff.base.seal.clone(),
    }
}

/// La evidencia que sostiene una relacion, como texto.
///
/// Es lo que compara cuando dos relaciones tienen la misma identidad y
/// distinto apoyo: el apoyo MAS PEQUENO gana, porque el resultado no
/// puede depender del orden en que las filas llegaron.
fn the_evidence_of(relation: &RelationFact) -> String {
    relation.evidence.clone().unwrap_or_default()
}

/// El recorrido en profundidad que busca las vueltas cerradas.
///
/// El camino lleva el nodo y la relacion por la que se llego a el: al
/// cerrarse la vuelta, las relaciones del ciclo son las del tramo del
/// camino mas la que la cierra.
fn visit(
    node: &str,
    edges: &BTreeMap<String, Vec<(String, String)>>,
    visited: &mut BTreeSet<String>,
    on_the_path: &mut BTreeSet<String>,
    path: &mut Vec<(String, String)>,
    cycles: &mut BTreeSet<Vec<String>>,
) {
    visited.insert(node.to_string());
    on_the_path.insert(node.to_string());
    path.push((node.to_string(), String::new()));

    if let Some(list) = edges.get(node) {
        for (to, identity) in list {
            if on_the_path.contains(to) {
                // Una arista hacia atras: el camino se cierra.
                if let Some(start) = path.iter().position(|(name, _)| name == to) {
                    let mut cycle: Vec<String> = path[start + 1..]
                        .iter()
                        .map(|(_, identity)| identity.clone())
                        .collect();
                    cycle.push(identity.clone());
                    cycle.sort();
                    cycle.dedup();
                    cycles.insert(cycle);
                }
            } else if !visited.contains(to) {
                visit(to, edges, visited, on_the_path, path, cycles);
            }
        }
    }

    path.pop();
    on_the_path.remove(node);
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::domain::graph::RelationType;
    use hodei_evidence::domain::read_set::{ReadSet, ReadStamp};

    /// El sujeto de las pruebas.
    fn artefact() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
    }

    /// El componente de las pruebas.
    fn component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
    }

    /// Otro componente de las pruebas.
    fn other_component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
    }

    /// La relacion de un componente a otro.
    fn depends_on(from: &Hrn, to: &Hrn) -> RelationFact {
        RelationFact::new(RelationType::DependsOn, from.clone(), to.clone())
    }

    /// Un grafo SIN ciclo: una cadena de dependencias.
    fn the_chain() -> Vec<RelationFact> {
        vec![
            depends_on(&artefact(), &component()),
            depends_on(&component(), &other_component()),
        ]
    }

    /// Un grafo con DOS ciclos que se tocan.
    fn the_two_cycles() -> Vec<RelationFact> {
        vec![
            depends_on(&artefact(), &component()),
            depends_on(&component(), &artefact()),
            depends_on(&component(), &other_component()),
            depends_on(&other_component(), &component()),
        ]
    }

    /// Un grafo SIN ciclo no declara ningun ciclo.
    #[test]
    fn an_acyclic_graph_declares_no_cycle() {
        let cycles = the_cycles_of(&the_chain());
        assert!(
            cycles.is_empty(),
            "una cadena de dependencias no es un ciclo: {:?}",
            cycles
        );
    }

    /// Un grafo con dos ciclos que se tocan declara LOS DOS.
    ///
    /// Y no uno: un recorrido que encuentra el primer ciclo y se calla
    /// es un recorrido que no ha mirado el resto.
    #[test]
    fn a_graph_with_two_cycles_declares_both() {
        let cycles = the_cycles_of(&the_two_cycles());
        assert_eq!(cycles.len(), 2, "los dos ciclos: {:?}", cycles);
        for cycle in &cycles {
            assert!(
                cycle.relations.len() >= 2,
                "un ciclo tiene al menos dos relaciones: {:?}",
                cycle
            );
            assert!(
                !cycle.subjects.is_empty(),
                "y dice a quien le llega: {:?}",
                cycle
            );
        }
    }

    /// Una dependencia de uno consigo mismo es un ciclo.
    #[test]
    fn a_self_dependency_is_a_cycle() {
        let relations = vec![depends_on(&artefact(), &artefact())];
        let cycles = the_cycles_of(&relations);
        assert_eq!(cycles.len(), 1, "la vuelta a si mismo: {:?}", cycles);
    }

    /// Solo se declaran los ciclos que la hipotetica INTRODUCE.
    ///
    /// Un ciclo que ya estaba en el mundo real no es culpa de la
    /// simulacion, y declararlo seria echarle la culpa a la hipotetica.
    #[test]
    fn only_the_cycles_that_the_hypothesis_introduces_are_declared() {
        let mut relations = the_two_cycles();
        // La hipotetica anade una relacion que NO cierra ningun ciclo.
        let added = depends_on(&artefact(), &other_component());
        let hypothetical = vec![added.identity()];
        relations.push(added);

        assert!(
            the_cycles_introduced_by(&relations, &hypothetical).is_empty(),
            "los ciclos del mundo real no los introduce la hipotetica"
        );
    }

    /// Y un ciclo que la hipotetica CIERRA si se declara.
    #[test]
    fn the_cycle_that_the_hypothesis_closes_is_declared() {
        let mut relations = the_chain();
        let added = depends_on(&other_component(), &artefact());
        let hypothetical = vec![added.identity()];
        relations.push(added);

        let cycles = the_cycles_introduced_by(&relations, &hypothetical);
        assert_eq!(
            cycles.len(),
            1,
            "la vuelta que cierra la hipotetica: {:?}",
            cycles
        );
    }

    /// La misma relacion dicha dos veces es UN duplicado, y la que repite
    /// lo que ya esta en el mundo tambien.
    #[test]
    fn the_duplicates_are_declared() {
        let observed = vec![depends_on(&artefact(), &component())];
        let added = vec![
            depends_on(&artefact(), &other_component()),
            depends_on(&artefact(), &other_component()),
            depends_on(&artefact(), &component()),
        ];

        let duplicates = the_duplicates_of(&added, &observed);
        assert_eq!(duplicates.len(), 2, "los dos duplicados: {duplicates:?}");
        assert!(
            duplicates.contains(&depends_on(&artefact(), &other_component()).identity()),
            "la relacion repetida: {duplicates:?}"
        );
        assert!(
            duplicates.contains(&depends_on(&artefact(), &component()).identity()),
            "y la que ya estaba en el mundo: {duplicates:?}"
        );
    }

    /// Una hipotetica que SUSTITUYE no es un duplicado: es otra cosa.
    #[test]
    fn a_hypothesis_that_replaces_is_not_a_duplicate() {
        let observed = vec![depends_on(&artefact(), &component())];
        let added = vec![depends_on(&artefact(), &other_component())];

        assert!(
            the_duplicates_of(&added, &observed).is_empty(),
            "sustituir no es repetir"
        );
    }

    /// Anadir y sustituir la misma relacion a la vez es una
    /// contradiccion.
    #[test]
    fn the_contradictions_are_declared() {
        let added = vec![depends_on(&artefact(), &other_component())];
        let removed = vec![depends_on(&artefact(), &other_component())];

        assert_eq!(
            the_contradictions_of(&added, &removed),
            vec![depends_on(&artefact(), &other_component()).identity()],
            "la contradiccion se declara"
        );
    }

    /// Las relaciones normalizadas no tienen duplicados, y de dos
    /// relaciones con la misma identidad gana la que lleva la evidencia
    /// MAS PEQUENA.
    #[test]
    fn the_normalized_relations_have_no_duplicates() {
        let first = depends_on(&artefact(), &component()).with_evidence("evd-2");
        let second = depends_on(&artefact(), &component()).with_evidence("evd-1");

        let normalized = normalize_relations(&[first.clone(), second.clone()]);

        assert_eq!(
            normalized.len(),
            1,
            "la misma relacion es una: {normalized:?}"
        );
        assert_eq!(
            normalized[0].evidence,
            Some("evd-1".to_string()),
            "y gana la evidencia mas pequena: {normalized:?}"
        );
    }

    /// Y el orden de las relaciones normalizadas no depende del orden de
    /// entrada.
    #[test]
    fn the_normalized_relations_do_not_depend_on_the_order() {
        let one = depends_on(&artefact(), &component());
        let other = depends_on(&component(), &other_component());

        assert_eq!(
            normalize_relations(&[one.clone(), other.clone()]),
            normalize_relations(&[other, one]),
            "el orden de entrada no cambia el resultado"
        );
    }

    /// El manifiesto canonico no depende del orden en que se leyo.
    #[test]
    fn the_manifest_does_not_depend_on_the_order_of_the_reads() {
        let mut first = ReadSet::empty();
        first.read(ReadStamp::relation("depends_on|a|b", "evd-1"));
        first.read(ReadStamp::evidence("evd-1", "sha256:aaa"));

        let mut second = ReadSet::empty();
        second.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        second.read(ReadStamp::relation("depends_on|a|b", "evd-1"));

        assert_eq!(first.manifest(), second.manifest());
        assert_eq!(first.fingerprint(), second.fingerprint());
    }
}
