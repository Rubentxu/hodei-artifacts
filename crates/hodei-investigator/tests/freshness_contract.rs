//! El contrato de la frescura de una investigacion entregada.
//!
//! Un informe dice que leyo, con su manifiesto: los sellos de lo que leyo
//! y la version del mundo en que lo leyo. Ese informe es una afirmacion
//! con fecha, y la pregunta es si sigue siendo cierta.
//!
//! La respuesta es de TRES, y estas seis cosas se prometen y se comprueban:
//!
//! - Un informe cuyo mundo no se movio **sigue vigente**.
//! - Un cambio de la relacion que el informe sostiene lo hace **obsoleto**,
//!   y dice **cual** fue la referencia que cambio.
//! - Un cambio **ajeno** al read-set NO invalida el informe: lo que no se
//!   leyo no puede caducar lo que se leyo.
//! - Una fuente que **no se puede consultar** devuelve `Unknown`: nunca un
//!   `Current` inventado.
//! - Un informe que **no dice de que mundo hablo** no puede comprobarse, y
//!   dice que no se sabe.
//! - La evidencia que **cambio de digest** o **ya no esta** hace obsoleto
//!   el informe.

use hodei_investigator::freshness::{
    ChangeKind, ChangedReference, CurrentWorld, EVIDENCE_SOURCE, Freshness, GRAPH_SOURCE, judge,
};
use hodei_investigator::{ReadKind, ReadSet, ReadStamp};

/// El manifiesto de una lectura: la version del grafo que se leyo, una
/// relacion y la evidencia que la sostiene.
fn the_report() -> ReadSet {
    let mut read_set = ReadSet::empty();
    read_set.generation(GRAPH_SOURCE, 12);
    read_set.read(ReadStamp::relation("contains|a|b", "ev-1"));
    read_set.read(ReadStamp::evidence("ev-1", "sha256:aaa"));
    read_set
}

/// El mundo de ahora: el grafo no se movio y la evidencia sigue igual.
fn the_world() -> CurrentWorld {
    CurrentWorld {
        generations: vec![(GRAPH_SOURCE.to_string(), 12)],
        digests: vec![("ev-1".to_string(), "sha256:aaa".to_string())],
        changes: Vec::new(),
        unreachable: Vec::new(),
    }
}

/// El mundo en el que el grafo se movio y una relacion cambio.
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

/// Un informe cuyo mundo no se movio SIGUE VIGENTE.
#[test]
fn a_report_whose_world_did_not_move_is_still_current() {
    assert_eq!(judge(&the_report(), &the_world()), Freshness::Current);
}

/// Un cambio de la relacion que el informe sostiene lo hace OBSOLETO, y
/// dice cual fue la referencia que cambio.
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

/// Un cambio AJENO al read-set no invalida el informe: lo que no se leyo
/// no puede caducar lo que se leyo.
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

/// El mismo cambio en la MISMA relacion, pero de otro apoyo, tampoco
/// invalida: el apoyo que el informe leyo sigue ahi.
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

/// Una fuente que no se puede consultar dice que NO SE SABE: nunca un
/// `Current` inventado.
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

/// La evidencia que no se pudo consultar tampoco se invierte: es `Unknown`.
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

/// Un informe que no dice de que mundo hablo NO se puede comprobar, y lo
/// dice: no se sabe, y no "sigue vigente".
#[test]
fn a_report_without_a_version_cannot_be_checked() {
    let mut read_set = ReadSet::empty();
    read_set.read(ReadStamp::relation("contains|a|b", "ev-1"));

    match judge(&read_set, &the_world()) {
        Freshness::Unknown { .. } => {}
        other => panic!("un informe sin version no puede decir que sigue vigente: {other:?}"),
    }
}

/// La evidencia que cambio de digest hace obsoleto el informe: lo que se
/// leyo no es lo que hay.
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

/// La evidencia que ya no esta hace obsoleto el informe: el informe se
/// apoya en algo que no existe.
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

/// El verificador es PURO: el mismo manifiesto y el mismo mundo dan el
/// mismo veredicto, siempre.
#[test]
fn the_verdict_is_the_same_every_time() {
    let first = judge(&the_report(), &the_moved_world());
    let second = judge(&the_report(), &the_moved_world());
    assert_eq!(first, second);
}
