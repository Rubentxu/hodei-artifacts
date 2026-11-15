//! El contrato del commit de publicación y el outbox de auditoría (HAR-03).
//!
//! Las pruebas van contra SurrealDB de verdad, con el motor **persistente**,
//! por la misma razón que las del catálogo: el motor en memoria no
//! arbitra dos escrituras concurrentes del mismo registro, y una garantía
//! que solo se cumple en un motor no es una garantía del catálogo.
//!
//! Lo que se comprueba aquí es una sola cosa, dicha de tres maneras: **el
//! asset y su evento de auditoría tienen una frontera de commit**. O se
//! escriben los dos, o no se escribe ninguno. Y lo que se comprueba del
//! lado de la entrega es que un evento que no se entregó **no se declara
//! entregado**, y que reentregarlo no duplica el efecto.

use std::sync::Arc;

use hodei_audit::{
    AuditContext, AuditDispatcher, AuditEvent, AuditOutbox, AuditResult, AuditSink,
    FailingAuditSink, InMemoryAuditSink,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogReader, CatalogWriter, DigestScope,
    PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::{SurrealAuditOutbox, SurrealCatalogStore};
use hodei_evidence::ContentDigest;
use kernel::application::ports::clock::{CountingIdGenerator, FixedClock, Identity};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

/// La cuenta con la que se publica en casi todas las pruebas.
const ACCOUNT: &str = "acme";

/// El actor que publica.
const ACTOR: &str = "hrn:hodei:iam:acme:User/alice";

/// La correlación de la petición.
const CORRELATION: &str = "corr-har03";

/// Una base en disco, con su namespace y su base de datos.
async fn open_store(path: &std::path::Path) -> Surreal<surrealdb::engine::local::Db> {
    let mut last = String::new();
    for _attempt in 0..20 {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    panic!("the database at {path:?} could not be opened: {last}");
}

/// Un almacén que deja pasar a las cuentas de la lista.
fn store_for(db: Surreal<surrealdb::engine::local::Db>) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer))
}

/// Un almacén con la identidad que el reloj y los identificadores.
///
/// El generador cuenta, así que el identificador del evento se sabe antes
/// de la publicación: sin eso no se puede provocar a propósito un choque
/// en la fila del outbox.
fn store_with_identity(
    db: Surreal<surrealdb::engine::local::Db>,
    identity: Identity,
) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer)).with_identity(identity)
}

/// La identidad que cuenta los identificadores.
fn counting_identity(prefix: &str) -> Identity {
    Identity::new(
        Arc::new(FixedClock::at("2026-11-15T00:00:00Z").expect("the moment")),
        Arc::new(CountingIdGenerator::with_prefix(prefix)),
    )
}

fn descriptor(account: &str, repo_key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn publication(repository: &str, asset_path: &str, content: &[u8]) -> Publication {
    Publication {
        repository: repository.to_string(),
        asset_path: asset_path.to_string(),
        coordinates: PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: "1.0.0".to_string(),
        },
        digest: ContentDigest::of_bytes(content),
        size_bytes: content.len() as u64,
        tags: Vec::new(),
        actor: ACTOR.to_string(),
        correlation_id: CORRELATION.to_string(),
    }
}

/// Un almacén con un repositorio ya creado.
async fn store_with_repository(
    path: &std::path::Path,
) -> (Surreal<surrealdb::engine::local::Db>, SurrealCatalogStore) {
    let db = open_store(path).await;
    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");
    store
        .create_repository(ACCOUNT, descriptor(ACCOUNT, "raw"))
        .await
        .expect("the repository is created");
    (db, store)
}

/// Cuántos assets hay en el catálogo.
async fn the_assets(db: &Surreal<surrealdb::engine::local::Db>) -> usize {
    let mut response = db
        .query("SELECT count() FROM catalog_asset GROUP ALL")
        .await
        .expect("the count");
    let rows: Vec<serde_json::Value> = response.take(0).expect("the count");
    rows.first()
        .and_then(|row| row.get("count"))
        .and_then(|count| count.as_u64())
        .unwrap_or(0) as usize
}

/// Cuántos eventos hay en el outbox.
async fn the_events(db: &Surreal<surrealdb::engine::local::Db>) -> usize {
    let mut response = db
        .query("SELECT count() FROM catalog_audit_outbox GROUP ALL")
        .await
        .expect("the count");
    let rows: Vec<serde_json::Value> = response.take(0).expect("the count");
    rows.first()
        .and_then(|row| row.get("count"))
        .and_then(|count| count.as_u64())
        .unwrap_or(0) as usize
}

/// Cuántas referencias a blobs hay.
///
/// La referencia es lo que impide que un blob se recoja: si la
/// publicación se cae, no queda ninguna, y el blob puede quedarse *staged*
/// sin que nada lo reclame. Eso es reconciliable; lo que no lo sería es un
/// asset publicado sin su evento.
async fn the_references(db: &Surreal<surrealdb::engine::local::Db>) -> usize {
    let mut response = db
        .query("SELECT count() FROM catalog_reference GROUP ALL")
        .await
        .expect("the count");
    let rows: Vec<serde_json::Value> = response.take(0).expect("the count");
    rows.first()
        .and_then(|row| row.get("count"))
        .and_then(|count| count.as_u64())
        .unwrap_or(0) as usize
}

/// El evento del outbox, el primero.
async fn the_event(db: &Surreal<surrealdb::engine::local::Db>) -> Option<AuditEvent> {
    let outbox = SurrealAuditOutbox::new(db.clone());
    let pending = outbox
        .pending(chrono::Utc::now())
        .await
        .expect("the pending");
    pending.into_iter().next().map(|entry| entry.event)
}

/// El evento pendiente **con el resultado pedido**.
///
/// "El primero" no es una pregunta con respuesta: los identificadores son
/// aleatorios, así que el orden de los pendientes cambia entre
/// ejecuciones, y una prueba que depende de ese orden pasa la mitad de las
/// veces y falla la otra mitad. La pregunta que sí tiene respuesta es
/// "¿qué evento dice esto?".
async fn the_event_with_result(
    db: &Surreal<surrealdb::engine::local::Db>,
    result: AuditResult,
) -> Option<AuditEvent> {
    let outbox = SurrealAuditOutbox::new(db.clone());
    let pending = outbox
        .pending(chrono::Utc::now())
        .await
        .expect("the pending");
    pending
        .into_iter()
        .map(|entry| entry.event)
        .find(|event| event.result == result)
}

// ---------------------------------------------------------------------------
// AC1: una sola frontera de commit.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_asset_and_its_audit_event_are_written_in_the_same_commit() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    // El asset está y el evento también: la misma frontera.
    let asset = store
        .asset(ACCOUNT, "raw", "app/one.bin")
        .await
        .expect("the asset");
    assert_eq!(asset.asset_path, "app/one.bin");
    assert_eq!(the_events(&db).await, 1);
}

#[tokio::test]
async fn an_asset_whose_audit_event_cannot_be_written_is_not_published() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_with_identity(db.clone(), counting_identity("event-"));
    store
        .create_repository(ACCOUNT, descriptor(ACCOUNT, "raw"))
        .await
        .expect("the repository is created");

    // Se ocupa la fila del outbox que la publicación va a escribir: la
    // escritura del evento va a fallar.
    let outbox = SurrealAuditOutbox::new(db.clone());
    outbox
        .put_pending(&the_event_with_id("event-0"))
        .await
        .expect("the event is said");

    let outcome = store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await;

    // La publicación no ocurrió: el evento no se pudo escribir, y el asset
    // tampoco se publica. Un asset sin su evento es un hecho sin rastro.
    assert!(
        outcome.is_err(),
        "the publication should not have succeeded"
    );
    assert_eq!(the_assets(&db).await, 0);
    let missing = store.asset(ACCOUNT, "raw", "app/one.bin").await;
    assert!(
        matches!(missing, Err(CatalogError::Missing { .. })),
        "the asset should not be there, and the error should say it is missing: {missing:?}"
    );

    // Y el blob no queda referido: la referencia es lo que impide que se
    // recoja, así que un blob sin referencia puede quedarse *staged* sin
    // que nada lo reclame. Eso es reconciliable; un asset publicado sin su
    // evento no lo es.
    assert_eq!(the_references(&db).await, 0);
}

#[tokio::test]
async fn a_publication_of_a_repository_that_does_not_exist_leaves_no_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db.clone());

    let outcome = store
        .publish(ACCOUNT, publication("ghost", "app/one.bin", b"one"))
        .await;

    assert!(outcome.is_err());
    assert_eq!(the_assets(&db).await, 0);
    assert_eq!(the_events(&db).await, 0);
}

// ---------------------------------------------------------------------------
// AC2: lo que lleva el evento, y lo que no.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_event_carries_the_actor_the_tenant_the_action_the_digest_and_the_correlation() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let event = the_event(&db).await.expect("the event");
    assert_eq!(event.actor(), ACTOR);
    assert_eq!(event.tenant(), ACCOUNT);
    assert_eq!(event.action, "publish");
    assert_eq!(event.resource, "raw/app/one.bin");
    assert_eq!(event.correlation_id(), CORRELATION);
    assert_eq!(event.result, AuditResult::Allowed);
    assert_eq!(
        event.digest.as_deref(),
        Some(
            format!(
                "{}:{}",
                ContentDigest::of_bytes(b"one").algorithm,
                ContentDigest::of_bytes(b"one").value
            )
            .as_str()
        )
    );
    assert!(!event.event_id.is_empty());
}

#[tokio::test]
async fn the_event_carries_no_secret() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let event = the_event(&db).await.expect("the event");
    let said = serde_json::to_string(&event).expect("the event as json");
    assert!(!said.contains("Bearer"), "the event says: {said}");
    assert!(!said.contains("token"), "the event says: {said}");
    assert!(!said.contains("secret"), "the event says: {said}");
}

#[tokio::test]
async fn a_redelivery_produces_exactly_one_logical_effect() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let sink = Arc::new(InMemoryAuditSink::new());
    let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

    // La caída: un worker reclama el evento y el sink lo recibe, pero la
    // marca no llega a escribirse. El lease expira y el evento vuelve al
    // mercado.
    let event = the_event(&db).await.expect("the event");
    let claimed = outbox
        .claim(
            &event.event_id,
            "worker-1",
            chrono::Utc::now() + chrono::Duration::minutes(1),
            chrono::Utc::now(),
        )
        .await
        .expect("the claim")
        .expect("the claim");
    sink.record(&claimed.event).expect("the sink received it");
    assert_eq!(sink.len(), 1);

    // La reentrega: el mismo evento, otro intento de entrega.
    let future = chrono::Utc::now() + chrono::Duration::minutes(10);
    let report = dispatcher
        .dispatch_pending("worker-2", future)
        .await
        .expect("the dispatch");
    assert_eq!(report.delivered(), 1);
    assert_eq!(
        sink.len(),
        1,
        "a re-delivery is the same event, so it is one logical effect"
    );
}

#[tokio::test]
async fn a_worker_that_lost_its_lease_does_not_produce_a_second_effect() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let event = the_event(&db).await.expect("the event");
    let now = chrono::Utc::now();

    // El worker-1 toma el evento y se queda con él más allá de su lease:
    // está lento, o su proceso está vivo pero no puede escribir.
    assert!(
        outbox
            .claim(
                &event.event_id,
                "worker-1",
                now + chrono::Duration::minutes(1),
                now
            )
            .await
            .expect("the claim")
            .is_some()
    );

    // El lease expira y el worker-2 lo toma: ahora es suyo.
    let later = now + chrono::Duration::minutes(2);
    assert!(
        outbox
            .claim(&event.event_id, "worker-2", later, later)
            .await
            .expect("the claim")
            .is_some()
    );

    // El worker-1 despierta e intenta devolver el evento al mercado: ya no
    // es suyo, y si pudiera, el evento volvería al mercado mientras el
    // worker-2 lo está entregando, y se entregaría dos veces.
    assert!(
        outbox
            .mark_undelivered(&event.event_id, "worker-1", "the sink is down")
            .await
            .is_err(),
        "the worker that lost the lease cannot return the event"
    );

    // El evento sigue siendo del worker-2: nadie se lo quitó.
    let entry = outbox
        .entry(&event.event_id)
        .await
        .expect("the entry")
        .expect("the entry");
    assert_eq!(entry.lease_owner.as_deref(), Some("worker-2"));
    assert_eq!(entry.state, hodei_audit::EntryState::Claimed);

    // Y el worker-2 lo entrega, una sola vez.
    let sink = Arc::new(InMemoryAuditSink::new());
    let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());
    let report = dispatcher
        .dispatch_pending("worker-2", later)
        .await
        .expect("the dispatch");
    assert_eq!(report.delivered(), 1);
    assert_eq!(sink.len(), 1);

    let entry = outbox
        .entry(&event.event_id)
        .await
        .expect("the entry")
        .expect("the entry");
    assert_eq!(entry.state, hodei_audit::EntryState::Delivered);
    assert_eq!(
        sink.len(),
        1,
        "one event, one logical effect, even with two workers"
    );
}

// ---------------------------------------------------------------------------
// AC3: lo que no se entregó no se declara entregado.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_failing_sink_leaves_the_event_pending_and_observable() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let sink = Arc::new(FailingAuditSink::new());
    let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

    let report = dispatcher
        .dispatch_pending("worker-1", chrono::Utc::now())
        .await
        .expect("the dispatch");

    assert_eq!(report.delivered(), 0);
    assert_eq!(report.not_delivered.len(), 1);

    // El evento sigue pendiente, con su intento contado y su error escrito:
    // lo que no se entregó tiene que poder mirarse.
    let entry = outbox
        .entry(&the_event(&db).await.expect("the event").event_id)
        .await
        .expect("the entry")
        .expect("the entry");
    assert_ne!(entry.state, hodei_audit::EntryState::Delivered);
    assert_eq!(entry.attempts, 1);
    assert!(entry.last_error.is_some());
    assert_eq!(sink.attempts(), 1);
}

#[tokio::test]
async fn an_event_that_was_never_claimed_cannot_be_marked_as_delivered() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = SurrealAuditOutbox::new(db.clone());
    let event = the_event(&db).await.expect("the event");

    // Nadie reclamó el evento: nadie lo entregó, y no se puede marcar.
    let marked = outbox
        .mark_delivered(&event.event_id, "worker-1", chrono::Utc::now())
        .await;
    assert!(marked.is_err());

    let entry = outbox
        .entry(&event.event_id)
        .await
        .expect("the entry")
        .expect("the entry");
    assert_ne!(entry.state, hodei_audit::EntryState::Delivered);
}

#[tokio::test]
async fn a_worker_whose_lease_expired_cannot_mark_the_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = SurrealAuditOutbox::new(db.clone());
    let event = the_event(&db).await.expect("the event");
    let now = chrono::Utc::now();
    let until = now + chrono::Duration::minutes(1);

    // El primer worker toma el evento.
    assert!(
        outbox
            .claim(&event.event_id, "worker-1", until, now)
            .await
            .expect("the claim")
            .is_some()
    );

    // Su lease muere y otro worker lo toma: el primero ya no puede decir
    // nada de él.
    let later = now + chrono::Duration::minutes(2);
    assert!(
        outbox
            .claim(
                &event.event_id,
                "worker-2",
                later + chrono::Duration::minutes(1),
                later
            )
            .await
            .expect("the claim")
            .is_some()
    );

    // Sin esta condición, el worker que perdió el lease podría marcar la
    // entrega del otro, o devolverlo al mercado mientras el otro lo
    // entrega: dos efectos lógicos de un solo evento.
    assert!(
        outbox
            .mark_delivered(&event.event_id, "worker-1", later)
            .await
            .is_err(),
        "the worker that lost the lease cannot mark a delivery"
    );
    assert!(
        outbox
            .mark_undelivered(&event.event_id, "worker-1", "the sink is down")
            .await
            .is_err(),
        "the worker that lost the lease cannot return the event"
    );

    // Y el evento sigue siendo del worker que lo tiene.
    let entry = outbox
        .entry(&event.event_id)
        .await
        .expect("the entry")
        .expect("the entry");
    assert_eq!(entry.lease_owner.as_deref(), Some("worker-2"));
    assert_eq!(entry.state, hodei_audit::EntryState::Claimed);
}

#[tokio::test]
async fn two_workers_claim_the_same_event_and_only_one_delivers() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let event = the_event(&db).await.expect("the event");
    let until = chrono::Utc::now() + chrono::Duration::minutes(1);

    let first = outbox
        .claim(&event.event_id, "worker-1", until, chrono::Utc::now())
        .await
        .expect("the claim");
    let second = outbox
        .claim(&event.event_id, "worker-2", until, chrono::Utc::now())
        .await
        .expect("the claim");

    assert!(first.is_some(), "the first worker takes the event");
    assert!(second.is_none(), "the second worker does not");
}

// ---------------------------------------------------------------------------
// AC4: caída antes y después del commit, y replay.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_crash_before_the_commit_leaves_no_asset_and_no_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_with_identity(db.clone(), counting_identity("event-"));
    store
        .create_repository(ACCOUNT, descriptor(ACCOUNT, "raw"))
        .await
        .expect("the repository is created");

    let outbox = SurrealAuditOutbox::new(db.clone());
    outbox
        .put_pending(&the_event_with_id("event-0"))
        .await
        .expect("the event is said");

    let outcome = store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await;
    assert!(outcome.is_err());

    // Ni el asset ni un evento nuevo: la transacción se cayó entera.
    assert_eq!(the_assets(&db).await, 0);
    assert_eq!(the_references(&db).await, 0);
    assert_eq!(
        the_events(&db).await,
        1,
        "only the event that was said before"
    );
}

#[tokio::test]
async fn a_crash_after_the_commit_leaves_the_event_pending_and_it_is_delivered_once() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    // El commit ocurrió: el asset está y el evento está pendiente.
    assert_eq!(the_assets(&db).await, 1);
    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let sink = Arc::new(InMemoryAuditSink::new());
    let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

    let report = dispatcher
        .dispatch_pending("worker-1", chrono::Utc::now())
        .await
        .expect("the dispatch");
    assert_eq!(report.delivered(), 1);
    assert_eq!(sink.len(), 1);
}

#[tokio::test]
async fn the_replay_of_the_outbox_does_not_duplicate_assets_nor_audit_events() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outbox = Arc::new(SurrealAuditOutbox::new(db.clone()));
    let sink = Arc::new(InMemoryAuditSink::new());
    let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

    dispatcher
        .dispatch_pending("worker-1", chrono::Utc::now())
        .await
        .expect("the dispatch");

    // El replay: se vuelve a entregar lo que ya está entregado, y no se
    // escribe ni un asset ni un evento de más.
    dispatcher
        .dispatch_pending("worker-1", chrono::Utc::now())
        .await
        .expect("the dispatch");

    assert_eq!(the_assets(&db).await, 1);
    assert_eq!(the_events(&db).await, 1);
    assert_eq!(sink.len(), 1);
}

#[tokio::test]
async fn a_store_written_by_an_older_schema_gets_the_outbox_table() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;

    // Se dice que el almacén lo escribió una build más vieja, sin la
    // tabla del outbox.
    db.query("UPSERT $id SET applied_version = $version")
        .bind((
            "id",
            surrealdb::sql::Thing::from(("catalog_schema", "applied")),
        ))
        .bind(("version", 1))
        .await
        .expect("write the version")
        .check()
        .expect("write the version");

    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");

    assert_eq!(
        store.applied_schema_version().await.expect("the version"),
        hodei_catalog_storage::SCHEMA_VERSION
    );

    // Y el outbox existe: la migración es idempotente y completa.
    store
        .create_repository(ACCOUNT, descriptor(ACCOUNT, "raw"))
        .await
        .expect("the repository is created");
    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");
    assert_eq!(the_events(&db).await, 1);
}

// ---------------------------------------------------------------------------
// Lo que la operación dice cuando no publica.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_idempotent_publication_does_not_write_a_second_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");
    let second = store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the republication");

    assert!(!second.created, "the republication did not write anything");
    assert_eq!(the_assets(&db).await, 1);
    assert_eq!(
        the_events(&db).await,
        1,
        "a republication that writes nothing does not say a new fact"
    );
}

#[tokio::test]
async fn a_conflicting_publication_leaves_a_conflict_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let (db, store) = store_with_repository(&tmp.path().join("db")).await;

    store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await
        .expect("the publication");

    let outcome = store
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"another"))
        .await;
    assert!(matches!(outcome, Err(CatalogError::Conflict { .. })));

    // El conflicto se dice: un rechazo que no deja rastro es
    // indistinguible de una petición que nunca ocurrió.
    let event = the_event_with_result(&db, AuditResult::Conflict)
        .await
        .expect("the conflict event");
    assert_eq!(event.result, AuditResult::Conflict);
    assert_eq!(the_assets(&db).await, 1);
    assert_eq!(the_events(&db).await, 2);
}

#[tokio::test]
async fn a_denied_publication_leaves_a_denied_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    // El repositorio se crea con un almacén que deja pasar; el que
    // publica no.
    let permissive = store_for(db.clone());
    permissive.initialize().await.expect("initialize");
    permissive
        .create_repository(ACCOUNT, descriptor(ACCOUNT, "raw"))
        .await
        .expect("the repository is created");

    let refusing = SurrealCatalogStore::with_authorizer(
        db.clone(),
        Arc::new(hodei_catalog::DenyAllAuthorizer),
    );

    let outcome = refusing
        .publish(ACCOUNT, publication("raw", "app/one.bin", b"one"))
        .await;
    assert!(matches!(outcome, Err(CatalogError::Denied { .. })));

    let event = the_event_with_result(&db, AuditResult::Denied)
        .await
        .expect("the denied event");
    assert_eq!(event.result, AuditResult::Denied);
    assert_eq!(the_assets(&db).await, 0);
}

/// Un evento con el identificador que la publicación va a usar.
fn the_event_with_id(event_id: &str) -> AuditEvent {
    AuditEvent::new(
        event_id.to_string(),
        AuditContext::new(ACTOR, ACCOUNT, CORRELATION),
        chrono::Utc::now(),
        "publish",
        "raw/app/one.bin",
        Some("sha256:0000".to_string()),
        AuditResult::Allowed,
    )
}
