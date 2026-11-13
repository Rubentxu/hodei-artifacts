//! The durable store of the reactive work, and the outbox of the
//! intents.
//!
//! The conformity of the port runs the SAME scenarios against the store
//! in memory and against the one in SurrealDB: one port, two materials.
//! And the durable one answers for the things that only matter when the
//! store survives a restart: the records, the receipts, the intents that
//! are said and not done.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_reactive::execution::{ExecutionOutcome, ExecutionRecord};
use hodei_reactive::intent::{Intent, IntentOutbox, Receipt};
use hodei_reactive::runtime::{ExecutionStore, InMemoryExecutionStore};
use hodei_trust_policy_storage::intents::SurrealIntentStore;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// Un registro con todo lo que le da valor: causalidad, intentos, traza
/// y un resultado de fallo que se puede reintentar.
fn a_record(key: &str, event: &str, cause: &str) -> ExecutionRecord {
    ExecutionRecord::new(
        key,
        "vulnerability_impact",
        event,
        vec![cause.to_string(), event.to_string()],
        Utc::now(),
    )
    .note("the guard passed", Utc::now())
    .finished(
        ExecutionOutcome::Failed {
            reason: "the read of the facts failed".to_string(),
            retryable: true,
        },
        Utc::now(),
    )
}

/// Un intent con todo lo que lo hace reconciliable.
fn an_intent(advisory: &str) -> Intent {
    Intent::new(
        "vulnerability_impact",
        &hodei_reactive::Event::new(
            format!("evt-{advisory}"),
            hodei_reactive::EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": advisory}),
            Utc::now(),
        ),
        &[format!("evt-{advisory}")],
        "acme",
        "security_case",
        format!("case:{advisory}:app-1"),
        "evidence:1.0.0",
        serde_json::json!({"advisory": advisory}),
    )
}

/// La conformidad del puerto: los mismos hechos, en el mismo orden, con
/// las mismas claves, en el material que sea.
async fn the_port_conforms(store: Arc<dyn ExecutionStore>) {
    let record = a_record("vulnerability_impact:evt-1", "evt-1", "evt-0");
    store.put(record.clone()).await;

    let read = store
        .execution_of("vulnerability_impact:evt-1")
        .await
        .expect("el registro queda");
    assert_eq!(read.key, record.key);
    assert_eq!(read.event_id, "evt-1");
    assert_eq!(read.causation, vec!["evt-0".to_string(), "evt-1".to_string()]);
    assert_eq!(read.attempts, record.attempts);
    assert_eq!(read.trace, record.trace, "la traza sobrevive");
    assert_eq!(read.outcome, record.outcome, "el resultado sobrevive");

    // La misma clave, otra vez: el registro se sustituye, no se duplica.
    let again = ExecutionRecord::new(
        "vulnerability_impact:evt-1",
        "vulnerability_impact",
        "evt-1",
        vec![],
        Utc::now(),
    )
    .finished(ExecutionOutcome::Completed, Utc::now());
    store.put(again).await;
    let read = store.execution_of("vulnerability_impact:evt-1").await.unwrap();
    assert_eq!(read.outcome, ExecutionOutcome::Completed);

    // Los efectos se deduplican por su clave.
    let effect = hodei_reactive::behavior::Effect::new("security_case", "case:1", serde_json::json!({"one": 1}));
    store.record_effect(effect.clone()).await;
    store.record_effect(effect.clone()).await;
    let effects = store.applied_effects().await;
    assert_eq!(effects.len(), 1, "el mismo efecto no se repite");

    // Y otro efecto distinto entra.
    store
        .record_effect(hodei_reactive::behavior::Effect::new(
            "decision",
            "decision:1",
            serde_json::json!({"two": 2}),
        ))
        .await;
    assert_eq!(store.applied_effects().await.len(), 2);

    // Las ejecuciones se leen, todas.
    let executions = store.executions().await;
    assert_eq!(executions.len(), 1, "una clave, un registro");
}

/// El puerto conforme en memoria.
#[tokio::test]
async fn the_port_conforms_in_memory() {
    the_port_conforms(Arc::new(InMemoryExecutionStore::empty())).await;
}

/// El puerto conforme en SurrealDB: las mismas respuestas, y sobrevive
/// al reinicio.
#[tokio::test]
async fn the_port_conforms_in_surreal() {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base de datos abre"),
    );
    db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
    the_port_conforms(Arc::new(SurrealIntentStore::new(db))).await;
}

/// El reinicio: los registros y los efectos estan donde estaban, con su
/// causalidad, sin depender de la memoria del proceso.
#[tokio::test]
async fn a_restart_recovers_the_records_and_the_effects() {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let path = tmp.path().join("db").to_string_lossy().to_string();

    {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.clone()).await.expect("la base abre"),
        );
        db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
        let store = SurrealIntentStore::new(db.clone());
        store
            .put(a_record("vulnerability_impact:evt-1", "evt-1", "evt-0"))
            .await;
        store
            .record_effect(hodei_reactive::behavior::Effect::new(
                "security_case",
                "case:1",
                serde_json::json!({"advisory": "GHSA-1"}),
            ))
            .await;
        // El proceso "cae": el store se deja caer aqui, y la base con
        // el, para que el lock del fichero se suelte.
        drop(store);
        drop(db);
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // El proceso vuelve: otro store sobre LA MISMA base.
    let db = Arc::new(Surreal::new::<RocksDb>(path).await.expect("la base vuelve"));
    db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
    let store = SurrealIntentStore::new(db);

    let record = store
        .execution_of("vulnerability_impact:evt-1")
        .await
        .expect("el registro sobrevive al reinicio");
    assert_eq!(record.causation, vec!["evt-0".to_string(), "evt-1".to_string()]);
    assert_eq!(store.applied_effects().await.len(), 1, "el recibo sobrevive");
}

/// La reclamacion es un compare-and-set: dos workers piden el mismo
/// intent y UNO lo gana. El que pierde lo sabe, y no toca nada.
#[tokio::test]
async fn two_workers_do_not_win_the_same_claim() {
    let (_tmp, store) = the_store().await;
    let intent = an_intent("GHSA-claim");

    store.put_pending(&intent).await.expect("el intent se dice");

    let now = Utc::now();
    let winner = store
        .claim(intent.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre")
        .expect("el worker a lo gana");
    assert_eq!(winner.identity, intent.identity);

    let loser = store
        .claim(intent.identity.as_str(), "worker-b", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre");
    assert!(loser.is_none(), "el worker b lo pierde y lo sabe");

    // El recibo del ganador: aplicado, desde claimed.
    store
        .record_receipt(&intent.identity, &Receipt::Applied)
        .await
        .expect("el recibo del ganador entra");

    // Y un intent aplicado no vuelve a reclamarse ni se recupera.
    assert!(
        store
            .claim(intent.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
            .await
            .expect("la reclamacion corre")
            .is_none(),
        "un intent aplicado no se reclama"
    );
    assert!(
        store.recover_pending(now).await.expect("la recuperacion corre").is_empty(),
        "un intent aplicado no esta pendiente"
    );
}

/// Un recibo repetido no cambia nada: dos veces aplicado es una vez
/// aplicado.
#[tokio::test]
async fn a_repeated_receipt_changes_nothing() {
    let (_tmp, store) = the_store().await;
    let intent = an_intent("GHSA-twice");

    store.put_pending(&intent).await.expect("el intent se dice");
    let now = Utc::now();
    store
        .claim(intent.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre")
        .expect("el worker lo gana");

    store.record_receipt(&intent.identity, &Receipt::Applied).await.expect("el recibo entra");
    store.record_receipt(&intent.identity, &Receipt::Applied).await.expect("el mismo recibo no rompe");

    assert!(
        store.recover_pending(now).await.expect("la recuperacion corre").is_empty(),
        "sigue aplicado, una sola vez"
    );
}

/// Un recibo para un intent que nadie reclamo no entra: un recibo sin
/// reclamacion no lo puede explicar nadie.
#[tokio::test]
async fn a_receipt_without_a_claim_is_refused() {
    let (_tmp, store) = the_store().await;
    let intent = an_intent("GHSA-ghost");

    store.put_pending(&intent).await.expect("el intent se dice");

    let outcome = store
        .record_receipt(&intent.identity, &Receipt::Applied)
        .await;
    assert!(outcome.is_err(), "no se recibe lo que no se reclamo: {outcome:?}");
}

/// El lease vencido devuelve el intent al mercado: el worker que murio
/// con el intent en la mano no se lo lleva para siempre.
#[tokio::test]
async fn an_expired_lease_makes_the_intent_claimable_again() {
    let (_tmp, store) = the_store().await;
    let intent = an_intent("GHSA-lease");

    store.put_pending(&intent).await.expect("el intent se dice");
    let now = Utc::now();

    // Un lease que ya nacio vencido: el worker tomo el intent y murio
    // antes de que el lease empezara a valer.
    store
        .claim(intent.identity.as_str(), "worker-dead", now - Duration::seconds(1), now)
        .await
        .expect("la reclamacion corre")
        .expect("el worker muerto lo gana en su momento");

    // La recuperacion lo vuelve a poner en el mercado.
    let pending = store.recover_pending(now).await.expect("la recuperacion corre");
    assert_eq!(pending.len(), 1, "el intent de un lease vencido vuelve");
    assert_eq!(pending[0].identity, intent.identity);

    // Y otro worker lo reclama.
    assert!(
        store
            .claim(intent.identity.as_str(), "worker-b", now + Duration::seconds(30), now)
            .await
            .expect("la reclamacion corre")
            .is_some(),
        "el worker b lo reclama"
    );
}

/// Un fallo que se puede reintentar vuelve al mercado; uno que no, no
/// vuelve; y lo que NO SE SABE no se reejecuta: se reconcilia primero.
#[tokio::test]
async fn the_receipt_says_what_happens_next() {
    let (_tmp, store) = the_store().await;
    let now = Utc::now();

    // El fallo con reintento: vuelve a pending.
    let retryable = an_intent("GHSA-retry");
    store.put_pending(&retryable).await.expect("el intent se dice");
    store
        .claim(retryable.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre")
        .expect("lo gana");
    store
        .record_receipt(
            &retryable.identity,
            &Receipt::Failed {
                reason: "the case store is down".to_string(),
                retryable: true,
            },
        )
        .await
        .expect("el recibo entra");
    let pending = store.recover_pending(now).await.expect("la recuperacion corre");
    assert_eq!(pending.len(), 1, "el fallo que se puede reintentar vuelve");

    // El fallo sin reintento: se queda failed.
    let permanent = an_intent("GHSA-permanent");
    store.put_pending(&permanent).await.expect("el intent se dice");
    store
        .claim(permanent.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre")
        .expect("lo gana");
    store
        .record_receipt(
            &permanent.identity,
            &Receipt::Failed {
                reason: "the advisory is malformed".to_string(),
                retryable: false,
            },
        )
        .await
        .expect("el recibo entra");
    let pending = store.recover_pending(now).await.expect("la recuperacion corre");
    assert!(
        pending.iter().all(|one| one.identity != permanent.identity),
        "el fallo permanente no vuelve solo"
    );

    // Lo desconocido: NO se reejecuta hasta que alguien reconcilia.
    let unknown = an_intent("GHSA-unknown");
    store.put_pending(&unknown).await.expect("el intent se dice");
    store
        .claim(unknown.identity.as_str(), "worker-a", now + Duration::seconds(30), now)
        .await
        .expect("la reclamacion corre")
        .expect("lo gana");
    store
        .record_receipt(
            &unknown.identity,
            &Receipt::Unknown {
                reason: "the answer never came".to_string(),
            },
        )
        .await
        .expect("el recibo entra");
    let pending = store.recover_pending(now).await.expect("la recuperacion corre");
    assert!(
        pending.iter().all(|one| one.identity != unknown.identity),
        "lo desconocido no se reejecuta solo: {pending:?}"
    );

    // La reconciliacion, explicita, lo devuelve al mercado.
    store.reconcile(&unknown.identity).await.expect("la reconciliacion entra");
    let pending = store.recover_pending(now).await.expect("la recuperacion corre");
    assert_eq!(
        pending.iter().filter(|one| one.identity == unknown.identity).count(),
        1,
        "reconciliado, vuelve a intentarse"
    );
}

/// El store sobre una base desechable.
async fn the_store() -> (tempfile::TempDir, SurrealIntentStore<Db>) {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base de datos abre"),
    );
    db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
    (tmp, SurrealIntentStore::new(db))
}
