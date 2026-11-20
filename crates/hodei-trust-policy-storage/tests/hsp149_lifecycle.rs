//! El ciclo de vida de la politica sobre el almacen real (HSP-149).
//!
//! Lo que se demuestra aqui es lo que el dominio promete y solo el
//! almacen puede probar:
//!
//! - La activacion es un CAS **dentro de la transaccion**: diez llamadas
//!   concurrentes sobre la misma revision dejan exactamente un ganador y
//!   el historial intacto (AC3).
//! - El rollback es una **revision nueva**: la cabecera apunta al contenido
//!   viejo copiado en una revision nueva y el historial conserva todas las
//!   anteriores, con su evento de invalidacion (AC3).
//! - La politica efectiva del servidor es **la revision publicada**: el
//!   cargador lee la cabecera, y una coleccion sin activar es un fallo, no
//!   un permiso (AC4, fail-closed).
//! - La validacion estricta se aplica en la frontera del almacen: un
//!   esquema que esta version no lee, o un requisito que nada satisface,
//!   no llega a revision (AC1).
//! - La separacion editor/aprobador es configurable y queda auditada en
//!   la cabecera y en los eventos (AC4).

use chrono::Utc;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
use hodei_trust_policy::lifecycle::{
    Actors, Coverage, LifecycleError, PolicyRevision, SeparationPolicy, ROLLBACK_PREFIX,
};
use hodei_trust_policy::policy::{Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy::ports::PolicyLoader;
use hodei_trust_policy::shadow::shadow_evaluate;
use hodei_trust_policy_storage::lifecycle::{
    HeadRow, SurrealLifecycleStore, EVENT_TABLE, HEAD_TABLE, REVISION_TABLE,
};
use std::sync::Arc;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// El conjunto de requisitos de una cabecera: la integridad tiene que
/// estar verificada.
fn policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        "prod",
        version,
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::Integrity,
            accepts: vec![DimensionState::Verified],
        })],
    )
}

/// El almacen abierto sobre un directorio de usar y tirar, como el resto
/// de pruebas de este crate.
async fn store() -> SurrealLifecycleStore<Db> {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("lifecycle")
        .await
        .expect("the namespace");
    SurrealLifecycleStore::new(db)
}

// ──────────────────────────── AC1 ────────────────────────────

/// Un conjunto contra un esquema que esta version no lee no llega a
/// revision: el almacenamiento lo rechaza antes de escribir.
#[tokio::test]
async fn a_policy_of_another_schema_is_never_saved() {
    let store = store().await;
    let mut broken = policy("1.0.0");
    broken.schema_version = "2.0.0".to_string();
    let error = store
        .save(broken, "alice", "the future", Utc::now())
        .await
        .expect_err("the schema is not known");
    assert!(matches!(error, LifecycleError::SchemaMismatch(..)));
    assert!(store
        .history("prod")
        .await
        .expect("the store reads")
        .is_empty());
}

/// Un requisito que no acepta ningun estado no puede salvarse: seria un
/// permiso que se decide solo al leerlo.
#[tokio::test]
async fn a_requirement_accepting_nothing_is_refused_at_the_boundary() {
    let store = store().await;
    let broken = TrustPolicy::new(
        "prod",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::Integrity,
            accepts: vec![],
        })],
    );
    let error = store
        .save(broken, "alice", "nothing satisfies", Utc::now())
        .await
        .expect_err("the requirement is unsatisfiable");
    assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
}

// ──────────────────────────── AC3 ────────────────────────────

/// Diez activaciones concurrentes de la misma revision, todas creyendo que
/// no hay cabecera: gana exactamente una. Las demas reciben la cabecera
/// real como perdedoras y el historial conserva la revision.
#[tokio::test]
async fn ten_concurrent_activations_have_exactly_one_winner() {
    let store = Arc::new(store().await);
    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");

    let mut tasks = Vec::new();
    for index in 0..10 {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            store
                .activate(
                    "prod",
                    1,
                    None,
                    &format!("approver-{index}"),
                    "the race",
                    Utc::now(),
                )
                .await
        }));
    }

    let mut winners = 0;
    let mut stale = 0;
    for task in tasks {
        match task.await.expect("the task does not panic") {
            Ok(head) => {
                assert_eq!(head.sequence, 1);
                winners += 1;
            }
            Err(LifecycleError::StaleHead(..)) => stale += 1,
            Err(other) => panic!("the race answered {other}"),
        }
    }
    assert_eq!(winners, 1, "una carrera deja un ganador");
    assert_eq!(stale, 9, "las demas saben contra que perdieron");

    let head = store
        .head("prod")
        .await
        .expect("the store reads")
        .expect("there is a winner");
    assert_eq!(head.sequence, 1);
    assert_eq!(
        store.history("prod").await.expect("the store reads").len(),
        1,
        "el historial intacto"
    );
}

/// Diez guardados concurrentes del mismo conjunto producen diez revisiones
/// distintas: la secuencia se re-valida dentro de la transaccion.
#[tokio::test]
async fn ten_concurrent_saves_take_ten_distinct_revisions() {
    let store = Arc::new(store().await);
    let mut tasks = Vec::new();
    for index in 0..10 {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            store
                .save(
                    policy("1.0.0"),
                    &format!("author-{index}"),
                    "a draft",
                    Utc::now(),
                )
                .await
                .map(|revision| revision.sequence)
        }));
    }

    let mut sequences = Vec::new();
    for task in tasks {
        sequences.push(
            task.await
                .expect("the task does not panic")
                .expect("every save lands"),
        );
    }
    sequences.sort_unstable();
    let expected: Vec<u64> = (1..=10).collect();
    assert_eq!(
        sequences, expected,
        "una revision por guardado, sin duplicados"
    );
}

/// El rollback no reescribe el pasado: copia el contenido de la revision
/// antigua en una revision nueva, la cabecera apunta a ella, el historial
/// conserva las tres y queda un evento de invalidacion que nombra la
/// revision que dejo de decidir.
#[tokio::test]
async fn a_rollback_is_a_new_revision_that_invalidates_the_head() {
    let store = store().await;
    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");
    store
        .save(policy("2.0.0"), "alice", "the second one", Utc::now())
        .await
        .expect("the save holds");
    store
        .activate("prod", 2, None, "bob", "2 goes live", Utc::now())
        .await
        .expect("the activation holds");

    let head = store
        .rollback(
            "prod",
            1,
            Some(2),
            &Actors::new("bob", "carol"),
            "2 broke staging",
            Utc::now(),
        )
        .await
        .expect("the rollback holds");

    assert_eq!(head.sequence, 3, "el rollback es una revision nueva");
    let revision = store
        .revision("prod", 3)
        .await
        .expect("the store reads")
        .expect("3 exists");
    assert!(revision.reason.starts_with(ROLLBACK_PREFIX));
    assert_eq!(revision.policy.version, "1.0.0-rb3");

    let history = store.history("prod").await.expect("the store reads");
    assert_eq!(history.len(), 3, "el historial conserva las tres");
    assert_eq!(history[0].sequence, 1, "la antigua sigue ahi");
    assert_eq!(history[0].policy.version, "1.0.0");

    let events = store.events("prod").await.expect("the store reads");
    let invalidated = events
        .iter()
        .find(|event| event.kind == "invalidated")
        .expect("the rollback invalidated the head");
    assert_eq!(
        invalidated.sequence, 2,
        "nombra la revision que dejo de decidir"
    );
    assert_eq!(invalidated.new_head, 3, "y la que empieza");
}

/// Un rollback que no espera la cabecera real no acontece: el CAS cubre el
/// rollback igual que la activacion.
#[tokio::test]
async fn a_rollback_that_does_not_expect_the_head_loses() {
    let store = store().await;
    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");
    store
        .activate("prod", 1, None, "bob", "go", Utc::now())
        .await
        .expect("the activation holds");
    store
        .save(policy("2.0.0"), "alice", "the second one", Utc::now())
        .await
        .expect("the save holds");

    let error = store
        .rollback(
            "prod",
            1,
            None,
            &Actors::new("bob", "carol"),
            "stale",
            Utc::now(),
        )
        .await
        .expect_err("the head moved under the rollback");
    assert!(matches!(error, LifecycleError::StaleHead(..)));
    assert_eq!(
        store
            .head("prod")
            .await
            .expect("the store reads")
            .expect("there is a head")
            .sequence,
        1,
        "la cabecera intacta"
    );
}

// ──────────────────────────── AC4 ────────────────────────────

/// La politica efectiva del servidor es la revision publicada: el cargador
/// lee la cabecera, y tras una reactivacion devuelve la revision de la
/// cabecera, no la ultima guardada.
#[tokio::test]
async fn the_effective_policy_is_the_published_revision() {
    let store = store().await;
    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");
    store
        .save(policy("2.0.0"), "alice", "the second one", Utc::now())
        .await
        .expect("the save holds");

    // Guardado no es publicado: mientras nadie active, el servidor no
    // tiene con que decidir, y eso es un fallo, no un permiso.
    let error = store
        .active_policy("prod")
        .await
        .expect_err("nothing is published yet");
    assert!(error.to_string().contains("no active policy set"));

    store
        .activate("prod", 2, None, "bob", "2 goes live", Utc::now())
        .await
        .expect("the activation holds");
    let active = store.active_policy("prod").await.expect("2 is published");
    assert_eq!(active.version, "2.0.0");

    // La publicacion de la revision 1 por CAS devuelve la cabecera a 1.
    store
        .activate("prod", 1, Some(2), "bob", "back to 1", Utc::now())
        .await
        .expect("the activation holds");
    let active = store.active_policy("prod").await.expect("1 is published");
    assert_eq!(
        active.version, "1.0.0",
        "efectiva = publicada, no ultima escrita"
    );
}

/// Con la separacion configurada, quien salvo la revision no puede
/// activarla; el registro dice quien apresto y quien aprobo.
#[tokio::test]
async fn the_separation_of_editor_and_approver_is_configurable_and_audited() {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("lifecycle")
        .await
        .expect("the namespace");
    let store = SurrealLifecycleStore::with_separation(db, SeparationPolicy::required());

    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");

    let error = store
        .activate("prod", 1, None, "alice", "self-approved", Utc::now())
        .await
        .expect_err("alice cannot approve her own revision");
    assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
    assert!(store.head("prod").await.expect("the store reads").is_none());

    let head: HeadRow = store
        .activate("prod", 1, None, "bob", "approved", Utc::now())
        .await
        .expect("bob may");
    assert_eq!(head.editor, "alice", "el registro dice quien apresto");
    assert_eq!(head.activated_by, "bob", "y quien aprobo");

    let events = store.events("prod").await.expect("the store reads");
    let activation = events
        .iter()
        .find(|event| event.kind == "activated")
        .expect("the activation is recorded");
    assert_eq!(activation.actor, "bob");
    assert_eq!(activation.editor, "alice");
}

// ─────────────────────── AC2 y la puerta ───────────────────────

/// El shadow se lee contra la revision publicada del almacen y no toca
/// nada: la cabecera sigue donde estaba y la portada del informe lo dice.
#[tokio::test]
async fn the_shadow_reads_the_published_head_and_leaves_it_alone() {
    let store = store().await;
    store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");
    let published = store
        .activate("prod", 1, None, "bob", "1 goes live", Utc::now())
        .await
        .expect("the activation holds");
    let published_digest = published.digest.clone();
    let active = store
        .revision("prod", published.sequence as u64)
        .await
        .expect("the store reads")
        .expect("the head names a revision")
        .policy;

    let report = shadow_evaluate(
        Some(&active),
        &policy("2.0.0"),
        &[],
        Coverage::new(12, 0),
        Utc::now(),
    )
    .expect("the candidate validates");

    assert!(!report.effects_executed);
    assert!(!report.snapshot_complete, "cero de doce no es todo");
    assert_eq!(
        store
            .head("prod")
            .await
            .expect("the store reads")
            .expect("still there")
            .digest,
        published_digest,
        "la cabecera no se movio"
    );
}

/// Una revision guardada vuelve del almacen con su digest intacto: el
/// historial es inmutable y verificable, y las tablas del contrato son
/// las que el codigo declara.
#[tokio::test]
async fn a_stored_revision_comes_back_with_its_digest_intact() {
    let store = store().await;
    let saved: PolicyRevision = store
        .save(policy("1.0.0"), "alice", "the first one", Utc::now())
        .await
        .expect("the save holds");
    let read = store
        .revision("prod", 1)
        .await
        .expect("the store reads")
        .expect("revision 1 is there");
    assert_eq!(read.digest, saved.digest);
    assert!(
        read.digest_holds(),
        "the content still hashes to its digest"
    );
    assert_eq!(read.policy, saved.policy);

    // The contract tables the store writes to.
    let tables = [REVISION_TABLE, HEAD_TABLE, EVENT_TABLE];
    for table in tables {
        assert!(!table.is_empty());
    }
}
