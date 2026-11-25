#![cfg(not(feature = "legacy"))]
//! HSP-213: el rollback no depende de arrancar dos autoridades.
//!
//! AC3 del retiro, demostrado con la maquina de estados de HSP-212 en el
//! **arbol productivo**: este target solo declara sus tests compilado
//! `--no-default-features` — el mismo arbol que rechaza el perfil
//! embebido (`hsp213_profile_rejection`) y que empaqueta
//! `scripts/package-release.sh`. Que la maquinaria de rollback
//! (`src/recovery/cutover.rs`, sin feature) funcione ahi ya es parte de
//! la prueba: volver atras no necesita el motor embebido retirado, ni un
//! segundo proceso que lo arranque.
//!
//! Sobre ese hecho, el ensayo fija la invariante que el encargo pide:
//!
//! - En toda fase hay a lo sumo **una** autoridad escribible. Conmutado
//!   el destino, el origen esta congelado: sirve lecturas y **cuenta**
//!   cada escritura que el servicio viejo intentara. Devuelta la
//!   autoridad, el destino pasa a espera: escribir ahi seria el
//!   dual-write que la maquina hace inconstructible.
//! - Tras la primera escritura nueva, el rollback libre esta prohibido
//!   (`PostWriteRollbackForbidden`); el regreso es el forward recovery
//!   (congelar, exportar, replay con gate, token) y no pierde ninguna
//!   de las escrituras nuevas.
//! - El rollback libre pre-escritura restaura el origen **exactamente**
//!   (fingerprint identico al de la barrera) y deja el destino fuera de
//!   juego: no hay ventana de dos autoridades ni un instante.
//!
//! La siembra y el paquete de 31 filas son los fixtures certificados de
//! HSP-210/211/212; el arnes de `tests/support` no se usa —enlaza
//! surrealdb y este target vive justo donde esa dependencia ya no esta—.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hodei_artifacts_api::recovery::cutover::{
    Authority, AuthorityError, CutoverError, CutoverMachine, Phase,
};
use hodei_artifacts_api::recovery::import::{
    DigestReconciler, ImportOptions, MemorySink, Row, SetReconciler, import_export, read_batch,
    read_import_manifest,
};
use serde_json::Value;

const PACKAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hsp210-authorities"
);

/// Claves de las filas del fixture que llevan un digest de contenido.
const DIGEST_KEYS: [(&str, &str); 5] = [
    ("catalog_asset", "sha256"),
    ("catalog_reference", "sha256"),
    ("provenance_receipt", "digest"),
    ("trust_grant", "evidence"),
    ("catalog_audit_outbox", "payload_digest"),
];

fn package_dir() -> PathBuf {
    Path::new(PACKAGE).to_path_buf()
}

fn options() -> ImportOptions {
    ImportOptions {
        batch_rows: 4,
        ..ImportOptions::default()
    }
}

/// El CAS de la plataforma del ensayo: exactamente los digests que el
/// fixture referencia, leidos del propio paquete.
fn fixture_cas() -> Arc<dyn DigestReconciler> {
    let manifest = read_import_manifest(&package_dir()).unwrap();
    let mut digests: BTreeSet<String> = BTreeSet::new();
    for table in &manifest.tables {
        for batch in &table.batches {
            let rows: Vec<Row> = read_batch(&package_dir(), batch).unwrap();
            for row in rows {
                for (name, field) in DIGEST_KEYS {
                    if table.table == name
                        && let Some(Value::String(digest)) = row.get(field)
                    {
                        digests.insert(digest.clone());
                    }
                }
            }
        }
    }
    Arc::new(SetReconciler::new(digests))
}

/// El origen con trafico: el fixture importado limpio, 31 filas.
async fn seeded_origin() -> Arc<MemorySink> {
    let dir = tempfile::tempdir().unwrap();
    let sink: Arc<MemorySink> = Arc::new(MemorySink::new());
    let report = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_cas()),
        &options(),
        &dir.path().join(".seed-progress.json"),
    )
    .await
    .expect("the origin is seeded from the fixture");
    assert_eq!(report.created, 31);
    sink
}

/// Montar el ensayo: origen sembrado y activo, destino vacio en espera.
/// El workspace lo sostiene el llamante.
async fn rehearsal(workspace: &Path) -> CutoverMachine {
    let origin = Arc::new(Authority::origin("origin", seeded_origin().await));
    let destination = Arc::new(Authority::destination(
        "destination",
        Arc::new(MemorySink::new()),
    ));
    CutoverMachine::new(
        origin,
        destination,
        fixture_cas(),
        workspace.to_path_buf(),
        options(),
    )
    .expect("the rehearsal starts with origin active and destination on standby")
}

/// La invariante AC3: a lo sumo una autoridad escribible en cada fase.
fn assert_single_authority(machine: &CutoverMachine) {
    assert!(
        !(machine.origin().is_active() && machine.destination().is_active()),
        "split-brain in phase {}",
        machine.phase().name()
    );
}

/// El recorrido hasta el corte: barrera, snapshot, import, gate, token.
async fn cut_over(machine: &mut CutoverMachine) -> hodei_artifacts_api::recovery::CutoverToken {
    assert_single_authority(machine);
    machine
        .close_barrier()
        .await
        .expect("the barrier closes and freezes the origin");
    assert_single_authority(machine);
    machine
        .import_snapshot()
        .await
        .expect("the snapshot imports into the standby");
    assert_single_authority(machine);
    let token = machine.verify().await.expect("the gate verifies");
    machine.commit(&token).await.expect("the token switches");
    assert_single_authority(machine);
    token
}

/// Una fila del dominio de las fixtures: un tag del repositorio `main`.
fn a_tag(name: &str, version: &str) -> (String, Row) {
    let mut row = Row::new();
    row.insert("account".into(), Value::String("acme".into()));
    row.insert("repository".into(), Value::String("main".into()));
    row.insert("tag_name".into(), Value::String(name.into()));
    row.insert("target_version".into(), Value::String(version.into()));
    (format!("catalog_tag:acme|main|{name}"), row)
}

/// AC3: tras la primera escritura nueva el regreso es el forward
/// recovery, y en ninguna fase del trayecto hay dos autoridades.
#[tokio::test]
async fn the_post_write_rollback_never_runs_two_authorities() {
    let dir = tempfile::tempdir().expect("a workspace");
    let mut machine = rehearsal(dir.path()).await;
    let _token = cut_over(&mut machine).await;

    // Conmutado: el origen esta congelado. Sirve lecturas del momento de
    // la barrera y rechaza y cuenta cada escritura: el dual-write del
    // rollback "por si acaso" no es constructible.
    let sample = machine
        .origin()
        .read("catalog_tag:acme|main|stable")
        .await
        .unwrap();
    assert!(sample.is_some(), "the frozen origin still serves reads");
    let (identity, row) = a_tag("stray", "0.0.1");
    let error = machine
        .origin()
        .write(&identity, &row)
        .await
        .expect_err("the frozen origin refuses service writes");
    assert!(matches!(error, AuthorityError::Frozen { .. }), "{error}");
    assert_eq!(machine.origin().refused_writes(), 1);

    // La primera escritura nueva en la autoridad nueva: punto de no
    // retorno del rollback libre.
    let (identity, row) = a_tag("beta", "2.1.0");
    machine
        .write_new(&identity, &row)
        .await
        .expect("the new authority takes writes");
    assert_eq!(machine.phase(), Phase::CutOverLive);
    let error = machine
        .rollback()
        .await
        .expect_err("free rollback is dead after the first new write");
    assert!(
        matches!(&error, CutoverError::PostWriteRollbackForbidden { writes } if *writes == 1),
        "{error}"
    );

    // El regreso: congelar, exportar, replay con gate. La invariante se
    // comprueba en cada fase, incluida esta.
    machine
        .export_forward()
        .await
        .expect("freeze + export the new authority");
    assert_single_authority(&machine);
    let replay_token = machine
        .replay_to_origin()
        .await
        .expect("the replay verifies against the frozen destination");
    assert_single_authority(&machine);
    machine
        .rollback_with_replay(&replay_token)
        .await
        .expect("the validated replay token brings the authority home");
    assert_eq!(machine.phase(), Phase::RolledBack);
    assert_single_authority(&machine);

    // Devuelta la autoridad, el destino queda fuera de juego: tras el
    // forward recovery sigue congelado —la replica exportada es la
    // evidencia del replay—, sirve lecturas y rechaza escrituras. Lo que
    // importa al AC3 es eso, no la etiqueta: no hay dos escribibles ni un
    // instante, y el origen vuelve a admitir.
    let (identity, row) = a_tag("sneaky", "9.9.9");
    let error = machine
        .destination()
        .write(&identity, &row)
        .await
        .expect_err("the demoted destination cannot be written");
    assert!(
        matches!(error, AuthorityError::Frozen { .. }),
        "the destination demoted by forward recovery stays frozen as the replay evidence: {error}"
    );
    assert!(!machine.destination().is_active());
    let (identity, row) = a_tag("after-rollback", "1.2.3");
    machine
        .origin()
        .write(&identity, &row)
        .await
        .expect("the restored origin takes writes again");

    // Sin perdida silenciosa: la escritura nueva esta en el origen.
    assert!(
        machine
            .origin()
            .read("catalog_tag:acme|main|beta")
            .await
            .unwrap()
            .is_some(),
        "the replay lost the new write"
    );
}

/// AC3 en su mitad simple: el rollback pre-primera-escritura es exacto,
/// y tampoco aqui llegan a coexistir dos autoridades.
#[tokio::test]
async fn the_pre_write_rollback_restores_one_authority_exactly() {
    let dir = tempfile::tempdir().expect("a workspace");
    let mut machine = rehearsal(dir.path()).await;
    let before = machine
        .origin()
        .fingerprint()
        .await
        .expect("origin fingerprint before the rehearsal");
    cut_over(&mut machine).await;

    machine.rollback().await.expect("free pre-write rollback");
    assert_eq!(machine.phase(), Phase::RolledBack);
    assert_single_authority(&machine);
    assert!(machine.origin().is_active());
    assert!(!machine.destination().is_active());
    assert_eq!(
        machine.origin().fingerprint().await.unwrap(),
        before,
        "restore is exact: the origin never wrote since the barrier"
    );

    // El destino, devuelto a la espera: no escribe, no es la segunda
    // autoridad a la que "volver atras" tendria que levantar.
    let (identity, row) = a_tag("sneaky", "9.9.9");
    let error = machine
        .destination()
        .write(&identity, &row)
        .await
        .expect_err("the standby destination cannot be written");
    assert!(matches!(error, AuthorityError::NotActive { .. }), "{error}");
}

/// La prueba de que el ensayo corre donde no hay motor embebido: el
/// propio arbol del test. Que este modulo compile sin `legacy` ya afirma
/// que la maquina de rollback (`src/recovery/cutover.rs`) no depende del
/// lector retirado; la asercion del mensaje recuerda que, en ese mismo
/// arbol, la unica forma de volver al almacen local es la herramienta de
/// la ventana, no un segundo binario del servicio.
#[test]
fn the_rehearsal_tree_ships_no_embedded_engine() {
    assert!(hodei_artifacts_api::bootstrap::LEGACY_PROFILE_REFUSAL.contains("HSP-213"));
    assert!(hodei_artifacts_api::bootstrap::LEGACY_PROFILE_REFUSAL.contains("hodei-recovery"));
}
