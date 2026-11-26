//! The contract for the freshness of a delivered investigation.
//!
//! A report says what it read, with its manifest: the stamps of what it read
//! and the world version in which it read it. That report is a dated
//! claim, and the question is whether it is still true.
//!
//! The answer is one of THREE, and these six things are promised and checked:
//!
//! - A report whose world did not move **remains current**.
//! - A change to the relation the report supports makes it **stale**,
//!   and says **which** reference changed.
//! - A change **foreign** to the read-set does NOT invalidate the report: what was not
//!   read cannot expire what was read.
//! - A source that **cannot be queried** returns `Unknown`: never an invented
//!   `Current`.
//! - A report that **does not say which world it spoke of** cannot be checked, and
//!   says that it is not known.
//! - Evidence that **changed digest** or **is no longer there** makes the
//!   report stale.

use hodei_investigator::freshness::{
    ChangeKind, ChangedReference, CurrentWorld, EVIDENCE_SOURCE, Freshness, GRAPH_SOURCE, judge,
};
use hodei_investigator::{ReadKind, ReadSet, ReadStamp};

/// A read's manifest: the graph version that was read, a
/// relation and the evidence that supports it.
fn the_report() -> ReadSet {
    let mut read_set = ReadSet::empty();
    read_set.generation(GRAPH_SOURCE, 12);
    read_set.read(ReadStamp::relation("contains|a|b", "ev-1"));
    read_set.read(ReadStamp::evidence("ev-1", "sha256:aaa"));
    read_set
}

/// The world as of now: the graph did not move and the evidence is unchanged.
fn the_world() -> CurrentWorld {
    CurrentWorld {
        generations: vec![(GRAPH_SOURCE.to_string(), 12)],
        digests: vec![("ev-1".to_string(), "sha256:aaa".to_string())],
        changes: Vec::new(),
        unreachable: Vec::new(),
    }
}

/// The world in which the graph moved and a relation changed.
fn the_moved_world() -> CurrentWorld {
    CurrentWorld {
        generations: vec![(GRAPH_SOURCE.to_string(), 13)],
        digests: vec![("ev-1".to_string(), "sha256:aaa".to_string())],
        changes: vec![ChangedReference {
            source: GRAPH_SOURCE.to_string(),
            kind: ReadKind::Relation,
            resource: "contains|a|b".to_string(),
            evidence: "ev-1".to_string(),
            what: ChangeKind::Removed,
        }],
        unreachable: Vec::new(),
    }
}

/// A report whose world did not move REMAINS CURRENT.
#[test]
fn a_report_whose_world_did_not_move_is_still_current() {
    assert_eq!(judge(&the_report(), &the_world()), Freshness::Current);
}

/// A change to the relation the report supports makes it STALE, and
/// says which reference changed.
#[test]
fn a_change_of_the_relation_the_report_rests_on_makes_it_stale() {
    match judge(&the_report(), &the_moved_world()) {
        Freshness::Stale { changed } => {
            assert_eq!(changed.len(), 1, "una referencia cambio: {changed:?}");
            assert_eq!(changed[0].kind, ReadKind::Relation);
            assert_eq!(changed[0].resource, "contains|a|b");
            assert_eq!(changed[0].what, ChangeKind::Removed);
        }
        other => panic!("un informe cuyo apoyo se retiro esta obsoleto: {other:?}"),
    }
}

/// A change FOREIGN to the read-set does not invalidate the report: what was not read
/// cannot expire what was read.
#[test]
fn a_change_away_from_the_read_set_does_not_invalidate_the_report() {
    let mut world = the_moved_world();
    world.changes = vec![ChangedReference {
        source: GRAPH_SOURCE.to_string(),
        kind: ReadKind::Relation,
        resource: "contains|x|y".to_string(),
        evidence: "ev-9".to_string(),
        what: ChangeKind::Added,
    }];

    assert_eq!(judge(&the_report(), &world), Freshness::Current);
}

/// The same change on the SAME relation, but from another support, does not
/// invalidate either: the support the report read is still there.
#[test]
fn another_support_of_the_same_relation_does_not_invalidate_the_report() {
    let mut world = the_moved_world();
    world.changes = vec![ChangedReference {
        source: GRAPH_SOURCE.to_string(),
        kind: ReadKind::Relation,
        resource: "contains|a|b".to_string(),
        evidence: "ev-9".to_string(),
        what: ChangeKind::Added,
    }];

    assert_eq!(judge(&the_report(), &world), Freshness::Current);
}

/// A source that cannot be queried says it is NOT KNOWN: never an invented
/// `Current`.
#[test]
fn a_source_that_cannot_be_consulted_says_that_it_is_not_known() {
    let mut world = the_world();
    world.generations.clear();
    world.unreachable.push(GRAPH_SOURCE.to_string());

    match judge(&the_report(), &world) {
        Freshness::Unknown { reason } => assert!(
            !reason.is_empty(),
            "el motivo se dice: un motivo vacio no es un motivo"
        ),
        other => panic!("una fuente que no contesta no dice que el mundo este quieto: {other:?}"),
    }
}

/// Evidence that could not be queried is not inverted either: it is `Unknown`.
#[test]
fn evidence_that_cannot_be_consulted_is_unknown_and_not_gone() {
    let mut world = the_world();
    world.digests.clear();
    world.unreachable.push(EVIDENCE_SOURCE.to_string());

    match judge(&the_report(), &world) {
        Freshness::Unknown { .. } => {}
        other => panic!("una evidencia que no se pudo leer no esta ido: {other:?}"),
    }
}

/// A report that does not say which world it spoke of CANNOT be checked, and it
/// says so: not known, and not "remains current".
#[test]
fn a_report_without_a_version_cannot_be_checked() {
    let mut read_set = ReadSet::empty();
    read_set.read(ReadStamp::relation("contains|a|b", "ev-1"));

    match judge(&read_set, &the_world()) {
        Freshness::Unknown { .. } => {}
        other => panic!("un informe sin version no puede decir que sigue vigente: {other:?}"),
    }
}

/// Evidence that changed digest makes the report stale: what was
/// read is not what is there.
#[test]
fn evidence_whose_digest_changed_makes_the_report_stale() {
    let mut world = the_world();
    world.digests = vec![("ev-1".to_string(), "sha256:bbb".to_string())];

    match judge(&the_report(), &world) {
        Freshness::Stale { changed } => {
            assert_eq!(changed.len(), 1, "una referencia cambio: {changed:?}");
            assert_eq!(changed[0].kind, ReadKind::Evidence);
            assert_eq!(changed[0].resource, "ev-1");
            assert_eq!(changed[0].what, ChangeKind::Changed);
        }
        other => panic!("una evidencia que cambio es un informe obsoleto: {other:?}"),
    }
}

/// Evidence that is no longer there makes the report stale: the report
/// relies on something that does not exist.
#[test]
fn evidence_that_is_gone_makes_the_report_stale() {
    let mut world = the_world();
    world.digests.clear();

    match judge(&the_report(), &world) {
        Freshness::Stale { changed } => {
            assert_eq!(changed.len(), 1, "una referencia se fue: {changed:?}");
            assert_eq!(changed[0].kind, ReadKind::Evidence);
            assert_eq!(changed[0].resource, "ev-1");
            assert_eq!(changed[0].what, ChangeKind::Gone);
        }
        other => panic!("una evidencia que se fue es un informe obsoleto: {other:?}"),
    }
}

/// The verifier is PURE: the same manifest and the same world give the
/// same verdict, always.
#[test]
fn the_verdict_is_the_same_every_time() {
    let first = judge(&the_report(), &the_moved_world());
    let second = judge(&the_report(), &the_moved_world());
    assert_eq!(first, second);
}
