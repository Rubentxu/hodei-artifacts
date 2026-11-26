//! The pure comparison: what is ordered, what is deduplicated and what
//! is contradicted.
//!
//! The helpers here are **pure**: they receive relations, identities and
//! decisions, and return relations, identities and decisions. They do not read,
//! do not write, do not decide and do not carry policy objects: policy
//! is simulation data, and a helper that carried it would be a
//! helper that could not be compared with another.
//!
//! And they are functions and not methods because their shape does not depend on who
//! calls them: the same helper orders one fork's overlay and another's, and
//! the two results are compared.

use std::collections::{BTreeMap, BTreeSet};

use kernel::Hrn;

use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::hypothetical::RelationFact;

/// A cycle: the relations that close it and the subjects it touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    /// The cycle's relations, in canonical order.
    pub relations: Vec<String>,
    /// The cycle's subjects, in canonical order.
    pub subjects: Vec<Hrn>,
}

/// Normalized relations: without duplicates and in canonical order.
///
/// Deduplication is by IDENTITY --the type and the two ends--, and of
/// two relations with the same identity the one carrying the SMALLEST
/// evidence wins: the result cannot depend on the order in which the
/// store returned the rows, because a result that depends on the order
/// of the rows is not a result of the world.
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

/// Decisions in canonical order: by subject.
///
/// A decision diff is compared with another decision diff, and two
/// lists read in a different order are not the same list.
pub fn normalize_decisions(decisions: &[DecisionChange]) -> Vec<DecisionChange> {
    let mut decisions = decisions.to_vec();
    decisions.sort_by_key(|change| change.subject.to_string());
    decisions
}

/// The overlay's cycles: the loops that close.
///
/// A cycle is a set of relations that step on each other: A reaches
/// B and B returns to A.
///
/// And they are ALL the cycles, unfiltered: the filter for the ones the
/// hypothetical introduces is another function, because they are two different questions and
/// a single function that mixes them cannot be tested.
pub fn the_cycles_of(overlay: &[RelationFact]) -> Vec<Cycle> {
    let mut edges: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for relation in overlay {
        edges
            .entry(relation.from.to_string())
            .or_default()
            .push((relation.to.to_string(), relation.identity()));
    }
    // The traversal has to be the same with any input order:
    // a cycle that appears in one execution and not in another is not a cycle
    // of the overlay, it is a cycle of the traversal.
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

/// The cycles the HYPOTHETICAL introduces.
///
/// A cycle that was already in the real world is not introduced by the
/// simulation, and declaring it would be blaming the hypothetical. What
/// the simulation introduces is what it declares.
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

/// Duplicates: the identities that are stated more than once.
///
/// A hypothesis that repeats a relation already in the world says the
/// same thing twice, and a hypothesis that repeats another hypothesis does too.
/// Both are declared: a diff that keeps them without saying anything looks like a
/// diff that did not look.
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

/// Contradictions: what one hypothesis says and another denies.
///
/// A hypothesis that adds a relation and another that replaces it
/// contradict each other: one says the relation exists and the other says it does not.
/// The overlay cannot obey both, and the result says so instead of
/// picking one on its own.
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

/// The subjects of a relation, by its identity.
pub fn the_subjects_of(identity: &str, relations: &[RelationFact]) -> Vec<Hrn> {
    the_subjects_of_all(std::slice::from_ref(&identity.to_string()), relations)
}

/// The subjects of a set of relations, by their identities.
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

/// The canonical form of a diff: what two simulations of the same world
/// have to share.
///
/// The diff carries the read manifest, and that manifest carries the ORDER
/// in which it was read. Two simulations of the same world read in a different order
/// and so their manifests are equal but not identical. The canonical
/// form is the one compared: the already ordered manifest and its fingerprint.
///
/// The structural diff and the decisions already come out in canonical order, and
/// so they are copied as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalDiff {
    /// The relations diff.
    pub structural: StructuralDiff,
    /// The decisions that change, in canonical order.
    pub decisions: Vec<DecisionChange>,
    /// The read manifest, ordered.
    pub manifest: String,
    /// The manifest's fingerprint.
    pub fingerprint: String,
    /// The stamp of the content read.
    pub seal: String,
}

/// The canonical form of a diff.
pub fn canonical(diff: &SimulationDiff) -> CanonicalDiff {
    CanonicalDiff {
        structural: diff.structural.clone(),
        decisions: normalize_decisions(&diff.decisions),
        manifest: diff.base.read_set.manifest(),
        fingerprint: diff.base.fingerprint(),
        seal: diff.base.seal.clone(),
    }
}

/// The evidence that supports a relation, as text.
///
/// It is what is compared when two relations have the same identity and
/// different support: the SMALLEST support wins, because the result cannot
/// depend on the order in which the rows arrived.
fn the_evidence_of(relation: &RelationFact) -> String {
    relation.evidence.clone().unwrap_or_default()
}

/// The depth-first traversal that looks for closed loops.
///
/// The path carries the node and the relation through which it was reached: when
/// the loop closes, the cycle's relations are those of the path segment
/// plus the one that closes it.
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
                // A back edge: the path closes.
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

    /// The test subject.
    fn artefact() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
    }

    /// The test component.
    fn component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
    }

    /// Another test component.
    fn other_component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
    }

    /// The relation from one component to another.
    fn depends_on(from: &Hrn, to: &Hrn) -> RelationFact {
        RelationFact::new(RelationType::DependsOn, from.clone(), to.clone())
    }

    /// A graph WITHOUT a cycle: a chain of dependencies.
    fn the_chain() -> Vec<RelationFact> {
        vec![
            depends_on(&artefact(), &component()),
            depends_on(&component(), &other_component()),
        ]
    }

    /// A graph with TWO cycles that touch.
    fn the_two_cycles() -> Vec<RelationFact> {
        vec![
            depends_on(&artefact(), &component()),
            depends_on(&component(), &artefact()),
            depends_on(&component(), &other_component()),
            depends_on(&other_component(), &component()),
        ]
    }

    /// A graph WITHOUT a cycle declares no cycle.
    #[test]
    fn an_acyclic_graph_declares_no_cycle() {
        let cycles = the_cycles_of(&the_chain());
        assert!(
            cycles.is_empty(),
            "una cadena de dependencias no es un ciclo: {:?}",
            cycles
        );
    }

    /// A graph with two cycles that touch declares BOTH.
    ///
    /// And not one: a traversal that finds the first cycle and goes quiet
    /// is a traversal that has not looked at the rest.
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

    /// A self-dependency is a cycle.
    #[test]
    fn a_self_dependency_is_a_cycle() {
        let relations = vec![depends_on(&artefact(), &artefact())];
        let cycles = the_cycles_of(&relations);
        assert_eq!(cycles.len(), 1, "la vuelta a si mismo: {:?}", cycles);
    }

    /// Only the cycles the hypothetical INTRODUCES are declared.
    ///
    /// A cycle that was already in the real world is not the simulation's
    /// fault, and declaring it would be blaming the hypothetical.
    #[test]
    fn only_the_cycles_that_the_hypothesis_introduces_are_declared() {
        let mut relations = the_two_cycles();
        // The hypothetical adds a relation that closes NO cycle.
        let added = depends_on(&artefact(), &other_component());
        let hypothetical = vec![added.identity()];
        relations.push(added);

        assert!(
            the_cycles_introduced_by(&relations, &hypothetical).is_empty(),
            "los ciclos del mundo real no los introduce la hipotetica"
        );
    }

    /// And a cycle that the hypothetical CLOSES is declared.
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

    /// The same relation stated twice is ONE duplicate, and one that repeats
    /// what is already in the world too.
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

    /// A hypothetical that REPLACES is not a duplicate: it is something else.
    #[test]
    fn a_hypothesis_that_replaces_is_not_a_duplicate() {
        let observed = vec![depends_on(&artefact(), &component())];
        let added = vec![depends_on(&artefact(), &other_component())];

        assert!(
            the_duplicates_of(&added, &observed).is_empty(),
            "sustituir no es repetir"
        );
    }

    /// Adding and replacing the same relation at once is a
    /// contradiction.
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

    /// Normalized relations have no duplicates, and of two
    /// relations with the same identity the one carrying the
    /// SMALLEST evidence wins.
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

    /// And the order of the normalized relations does not depend on the input
    /// order.
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

    /// The canonical manifest does not depend on the order in which it was read.
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
