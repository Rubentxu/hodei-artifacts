//! El contrato comun de los backends del BlobStore.
//!
//! La suite se define una sola vez aqui; cada backend la pasa en su
//! propio fichero de test. Lo que se afirma de uno, se afirma del otro:
//! put/get/stat, digest, doble publicacion, error, cancelacion,
//! streaming y tramos.

// Los binarios de test comparten este modulo: cada uno usa una parte.
#![allow(dead_code)]

pub mod conformity;
pub mod localstack;
pub mod rustfs;

use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation, ReadPolicy};

/// El contenido de la suite: bytes distinguibles.
pub fn the_bytes() -> Vec<u8> {
    (0u8..=255).cycle().take(5000).collect()
}

/// put y get: lo que sale por el digest es exactamente lo que entro.
async fn case_put_and_get_roundtrip(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    assert_eq!(digest, ContentDigest::of_bytes(&bytes));
    let read_back = store.get(&digest).await.expect("el contenido se lee");
    assert_eq!(read_back, bytes, "lo que sale es lo que entro");
}

/// stat: existir y medir longitudes sin leer el contenido.
async fn case_stat(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    assert!(
        store
            .exists(&digest)
            .await
            .expect("existe se puede preguntar")
    );
    assert_eq!(
        store.length(&digest).await.expect("la longitud sin leer"),
        bytes.len() as u64
    );
    let absent = ContentDigest::of_bytes(b"never stored");
    assert!(
        !store
            .exists(&absent)
            .await
            .expect("no existe tambien se puede preguntar")
    );
    assert_eq!(
        store.length(&absent).await.unwrap_err(),
        BlobError::NotFound
    );
}

/// El digest manda: pedir por un digest que no existe es un fallo,
/// nunca un contenido ajeno.
async fn case_digest_is_the_identity(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    let read = store
        .get_with(&digest, ReadPolicy::Trust)
        .await
        .expect("confiar es leer sin recalcular");
    assert_eq!(read, bytes);
    store
        .verify(&digest)
        .await
        .expect("verificar lo bueno pasa");

    let absent = ContentDigest::of_bytes(b"absent content");
    assert_eq!(store.get(&absent).await.unwrap_err(), BlobError::NotFound);
    assert_eq!(
        store.verify(&absent).await.unwrap_err(),
        BlobError::NotFound
    );
}

/// Doble publicacion: el mismo contenido dos veces no es un error y no
/// guarda dos veces.
async fn case_double_publishing(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let first = store.put(&bytes).await.expect("la primera publicacion");
    let second = store.put(&bytes).await.expect("la segunda no es error");
    assert_eq!(first, second, "la identidad es el contenido");
    let read_back = store.get(&first).await.expect("el contenido se lee");
    assert_eq!(read_back, bytes, "y sigue siendo el mismo");
}

/// El commit valida: lo declarado manda, y lo que no cuadra no se publica.
async fn case_commit_validates(store: &dyn BlobStore) {
    // Contenido propio de este caso: los anteriores ya publicaron
    // `the_bytes()`, y preguntar por el de ellos no diria nada del commit.
    let bytes = b"content that only this case tries to commit".to_vec();
    let lie = CommitExpectation {
        digest: ContentDigest::of_bytes(b"another content entirely"),
        length: bytes.len() as u64,
    };
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&bytes).await.expect("los bytes entran");
    let error = session
        .commit(lie)
        .await
        .expect_err("un digest mentiroso no publica");
    assert!(
        matches!(error, BlobError::DigestMismatch { .. }),
        "la mentira del digest se llama por su nombre: {error}"
    );

    let lie_length = CommitExpectation {
        digest: ContentDigest::of_bytes(&bytes),
        length: bytes.len() as u64 + 1,
    };
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&bytes).await.expect("los bytes entran");
    let error = session
        .commit(lie_length)
        .await
        .expect_err("una longitud mentirosa no publica");
    assert!(
        matches!(error, BlobError::LengthMismatch { .. }),
        "la mentira de longitud se llama por su nombre: {error}"
    );

    let lie_algorithm = CommitExpectation {
        digest: ContentDigest {
            algorithm: DigestAlgorithm::Sha512,
            value: "ab".repeat(64),
        },
        length: bytes.len() as u64,
    };
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&bytes).await.expect("los bytes entran");
    let error = session
        .commit(lie_algorithm)
        .await
        .expect_err("un algoritmo que este almacen no calcula no publica");
    assert!(
        matches!(error, BlobError::UnsupportedAlgorithm(_)),
        "el algoritmo ajeno se llama por su nombre: {error}"
    );

    // Y ninguna de las tres negaciones dejo referencia visible.
    let uncommitted = ContentDigest::of_bytes(&bytes);
    assert!(
        !store
            .exists(&uncommitted)
            .await
            .expect("preguntar se puede"),
        "lo rechazado no existe: no quedo ninguna referencia visible"
    );
}

/// Cancelacion: abortar una sesion no publica nada, y la sesion muerta
/// no admite mas operaciones.
async fn case_cancellation(store: &dyn BlobStore) {
    // Contenido propio: otros casos ya publicaron `the_bytes()`, y
    // preguntar por el de ellos no diria nada de la cancelacion.
    let bytes = b"content this case only appends, and then aborts".to_vec();
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&bytes).await.expect("los bytes entran");
    session.abort().await.expect("abortar es limpio");
    let error = session.append(b"more").await.unwrap_err();
    assert_eq!(error, BlobError::SessionClosed, "muerta, no escribe");
    session
        .abort()
        .await
        .expect("abortar dos veces no es error");
    assert!(
        !store
            .exists(&ContentDigest::of_bytes(&bytes))
            .await
            .expect("preguntar se puede"),
        "lo abortado no existe"
    );
}

/// Streaming: el contenido entra por trozos y sale verificado por su
/// digest, que la sesion conoce antes del commit.
async fn case_streaming_session(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let mut session = store.begin().await.expect("la sesion abre");
    let mut sent = 0usize;
    while sent < bytes.len() {
        let end = (sent + 700).min(bytes.len());
        session
            .append(&bytes[sent..end])
            .await
            .expect("el trozo entra");
        sent = end;
    }
    assert_eq!(session.bytes_written(), bytes.len() as u64);
    let digest = session.the_digest();
    assert_eq!(
        digest,
        ContentDigest::of_bytes(&bytes),
        "la sesion lleva la cuenta justa"
    );
    let committed = session
        .commit(CommitExpectation::of(&bytes))
        .await
        .expect("el commit cuadra y publica");
    assert_eq!(committed, digest);
    assert_eq!(store.get(&committed).await.expect("y se puede leer"), bytes);
}

/// Tramos: leer una parte sin traer el todo, y el tramo que no existe se
/// dice con lo que hay.
async fn case_range_reads(store: &dyn BlobStore) {
    let bytes = the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    let range = store
        .read_range(&digest, 10, 5)
        .await
        .expect("el tramo se sirve");
    assert_eq!(range, bytes[10..15]);
    let error = store.read_range(&digest, 4995, 10).await.unwrap_err();
    assert_eq!(
        error,
        BlobError::LengthMismatch {
            expected: 10,
            found: 5
        },
        "el tramo que asoma del final no se sirve truncado"
    );
    let error = store.read_range(&digest, 10_000, 1).await.unwrap_err();
    assert!(
        matches!(
            error,
            BlobError::LengthMismatch { .. } | BlobError::NotFound
        ),
        "el offset pasado del final tampoco es un tramo: {error}"
    );
}

/// Cero bytes: el contenido vacio tambien tiene digest y tambien viaja.
async fn case_empty_content(store: &dyn BlobStore) {
    let digest = store.put(&[]).await.expect("lo vacio se guarda");
    assert_eq!(digest, ContentDigest::of_bytes(&[]));
    assert_eq!(store.get(&digest).await.expect("lo vacio se lee").len(), 0);
    assert_eq!(store.length(&digest).await.expect("lo vacio mide cero"), 0);
}

/// La suite entera, la misma para los dos backends.
pub async fn the_suite(store: &dyn BlobStore) {
    case_put_and_get_roundtrip(store).await;
    case_stat(store).await;
    case_digest_is_the_identity(store).await;
    case_double_publishing(store).await;
    case_commit_validates(store).await;
    case_cancellation(store).await;
    case_streaming_session(store).await;
    case_range_reads(store).await;
    case_empty_content(store).await;
}
