//! HSP-201: sesiones OCI reanudables entre dos réplicas sobre el
//! almacén de ficheros de verdad.
//!
//! El slice saca el registro de sesión de la memoria del proceso. Este
//! corte lo prueba sin servidor: un **journal** compartido (la autoridad
//! del offset), dos **registros** —las dos réplicas— y un
//! [`FileBlobStore`] de verdad en un directorio temporal.
//!
//! - **AC1**: una subida abierta en la réplica A continúa en la réplica
//!   B, y el `PUT` de cierre se hace en B: el offset y el digest del
//!   blob publicado son los de siempre. Un escritor rancio (replay) no
//!   mueve la autoridad; un staging con bytes por encima de la autoridad
//!   se recorta al reanudar, y un cierre cuya longitud no cuadra no
//!   publica nada.
//! - **AC2**: una sesión de otra cuenta no existe para esta: ni se
//!   reanuda ni su offset se lee desde otro ámbito.
//! - **AC3**: el barrido del TTL cierra la autoridad y aborta el staging
//!   huérfano de una sesión que otra réplica abandonó, sin publicar
//!   nunca bytes no verificados.
//!
//! El journal de Dgraph (la misma autoridad sobre el grafo real, con su
//! CAS arbitrado por el servidor y su barrido acotado al inquilino) se
//! prueba en `tests/hsp201_sessions_dgraph_it.rs`.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use hodei_artifacts_api::oci::session_journal::{MemoryJournal, SessionJournal};
use hodei_artifacts_api::oci::uploads::{Advance, UploadRegistry};
use hodei_artifacts_api::quota::{QuotaLedger, QuotaLimits};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobStore, CommitExpectation, FileBlobStore};

/// El contenido de un trozo: veinte kilobytes deterministas.
fn a_piece(seed: u8) -> Vec<u8> {
    (0u16..20_480)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// Una reserva sin límites: estas pruebas prueban sesiones, no cuotas.
fn an_unlimited_ticket(account: &str) -> hodei_artifacts_api::quota::QuotaTicket {
    QuotaLedger::new(QuotaLimits::default())
        .begin(account, None)
        .expect("an unlimited ledger admits everything")
}

/// El digest declarado de un contenido.
fn a_declared_digest(content: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(content)
}

/// Abre una sesión en la réplica, escribe un trozo y lo confiesa ante la
/// autoridad: el camino del `PATCH`, menos el alambre.
async fn write_a_piece(
    registry: &UploadRegistry,
    store: &FileBlobStore,
    account: &str,
    repository: &str,
    id: &str,
    version: u64,
    piece: &[u8],
) -> u64 {
    let session = registry
        .find_or_resume(account, repository, id, store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let durable = {
        let mut session = session.lock().await;
        let (writer, _ticket) = session
            .the_writer_and_ticket()
            .expect("the session has a writer");
        writer.append(piece).await.expect("the piece is written");
        session.flush_durable().await.expect("the piece is durable")
    };
    match registry.advance(id, version, durable).await {
        Advance::Admitted(next) => {
            let mut session = session.lock().await;
            session.set_journal_version(next);
            // La réplica suelta el mango: el staging es del journal, y
            // la próxima petición —aquí o en la otra réplica— lo toma.
            session.release_writer_when_possible();
            next
        }
        other => panic!("the authority refused the piece: {other:?}"),
    }
}

/// AC1 — lo que la réplica A empezó lo termina la réplica B.
#[tokio::test]
async fn two_replicas_publish_one_blob_through_the_authority() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let first = a_piece(1);
    let second = a_piece(2);
    let whole = [first.as_slice(), second.as_slice()].concat();

    // POST en A: la sesión nace en la autoridad y el mango se suelta.
    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // PATCH del primer trozo en A: escribe, la autoridad admite, el
    // mango vuelve a soltarse.
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &first).await;

    // PATCH del segundo trozo en B: B no tiene asiento; la autoridad
    // dice el offset y el almacén reabre el staging.
    write_a_piece(&replica_b, &store, "acme", "releases", &id, 1, &second).await;

    // PUT de cierre en B: el último mango commitea contra el digest
    // declarado. La sesión arrancó en otro proceso, así que el digest
    // local no cubre los primeros bytes; quien publica es el almacén,
    // y la promesa no cambia.
    let shared = replica_b
        .find_or_resume("acme", "releases", &id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let published = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session has a writer");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&whole),
                length: whole.len() as u64,
            })
            .await
            .expect("the commit publishes the whole blob")
    };
    assert_eq!(published, a_declared_digest(&whole));
    assert!(
        store.exists(&published).await.expect("the store answers"),
        "the blob is not there after a cross-replica publish"
    );
    assert_eq!(
        store.get(&published).await.expect("the blob reads"),
        whole,
        "the published bytes are not the bytes the two replicas wrote"
    );

    // La autoridad se quedó en el offset final y la sesión se cierra.
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands");
    assert_eq!(record.offset, whole.len() as u64);
    assert_eq!(record.version, 2);
    replica_b.close(&id).await.expect("the session closes");
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands closed");
    assert!(record.closed);
    assert!(
        replica_b
            .find_or_resume("acme", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
}

/// AC1 — un escritor rancio no mueve la autoridad y un staging por
/// encima de ella no duplica bytes al publicar.
#[tokio::test]
async fn a_stale_replay_never_duplicates_bytes() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let first = a_piece(1);
    let second = a_piece(2);

    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // El primer trozo se confiesa: autoridad en `first`, revision 1.
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &first).await;

    // Un replay con la revision vieja no mueve nada: es el `416` del
    // protocolo, y la autoridad sigue donde estaba.
    match replica_a.advance(&id, 0, 999_999).await {
        Advance::Conflict(offset) => assert_eq!(offset, first.len() as u64),
        other => panic!("the stale writer was not refused: {other:?}"),
    }

    // Y los bytes por encima de la autoridad se van al reanudar: un
    // staging mas largo que lo declarado se recorta, no se publica.
    // El handle sale de la autoridad, no de un asiento: la réplica A
    // ya solto su mango y no lo vuelve a tomar.
    let handle = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands")
        .handle;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&handle)
        .expect("the staging opens")
        .write_all(&second)
        .expect("a stray piece is scribbled past the authority");

    // La réplica B reanuda con la autoridad como verdad: lo que sobra
    // se corta y el cursor que B ve es el de la autoridad.
    let shared = replica_b
        .find_or_resume("acme", "releases", &id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let (cursor, covers) = {
        let session = shared.lock().await;
        (
            session
                .the_local_offset()
                .expect("the session has a local offset"),
            session.the_digest_covers_all(),
        )
    };
    assert_eq!(
        cursor,
        first.len() as u64,
        "the resume did not trim the stray bytes"
    );
    assert!(
        !covers,
        "a resumed session cannot claim a full local digest"
    );
    let published = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session resumed");
        writer
            .append(&second)
            .await
            .expect("the second piece is written");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&[first.as_slice(), second.as_slice()].concat()),
                length: first.len() as u64 + second.len() as u64,
            })
            .await
            .expect("the commit publishes exactly the authority's bytes")
    };
    assert_eq!(
        store.get(&published).await.expect("the blob reads"),
        [first.as_slice(), second.as_slice()].concat(),
        "a replayed piece reached the published blob"
    );

    // Un cierre cuya longitud no cuadra no publica: el almacén descarta.
    let session = replica_b
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let other_id = session.lock().await.the_id().to_string();
    drop(session);
    write_a_piece(&replica_b, &store, "acme", "releases", &other_id, 0, &first).await;
    let shared = replica_b
        .find_or_resume("acme", "releases", &other_id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let wrong = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session has a writer");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&first),
                length: first.len() as u64 + 7,
            })
            .await
    };
    assert!(
        wrong.is_err(),
        "a commit whose length does not match must not publish"
    );
    assert!(
        !store
            .exists(&a_declared_digest(&first))
            .await
            .expect("the store answers"),
        "a rejected commit left a published blob behind"
    );
}

/// AC2 — una sesión de otra cuenta no existe: no se reanuda ni se lee.
#[tokio::test]
async fn an_alien_session_is_invisible() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // Otra cuenta no la ve —ni siquiera en la otra réplica— y su offset
    // tampoco es legible desde su ambito.
    assert!(
        replica_b
            .find_or_resume("other", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
    assert!(
        replica_b
            .find_or_resume("acme", "other-repo", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
    assert_eq!(
        replica_b
            .the_journal_offset("other", "releases", &id)
            .await
            .expect("the journal answers"),
        None
    );
    // La dueña si la ve.
    assert!(
        replica_b
            .find_or_resume("acme", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_some()
    );
}

/// AC3 — el barrido reconcilia el staging huerfano y no publica nada.
#[tokio::test]
async fn the_ttl_sweep_aborts_the_orphan_without_publishing() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    // Una gracia de unos milisegundos: la subida no termina nunca.
    let ttl = Duration::from_millis(20);
    let replica_a = UploadRegistry::with_journal(ttl, 8, Arc::clone(&journal));
    // La "otra réplica" es el mismo journal sin asientos: es la
    // autoridad que sobrevivio al crash.
    let replica_b = UploadRegistry::with_journal(ttl, 8, Arc::clone(&journal));

    let piece = a_piece(3);
    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &piece).await;

    // El crash: la réplica B espera a que la gracia pase y barre.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let reconciled = replica_b.sweep(&store).await.expect("the sweep answers");
    assert!(
        reconciled >= 1,
        "the orphan of a dead replica was not reconciled"
    );

    // Nada publicado: el staging murio y el blob no existe.
    assert!(
        !store
            .exists(&a_declared_digest(&piece))
            .await
            .expect("the store answers"),
        "the sweep published unverified bytes"
    );
    // La autoridad la cerró el propio barrido.
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands closed");
    assert!(record.closed, "the swept session is still open");
}
