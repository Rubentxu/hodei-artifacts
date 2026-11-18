//! La conformidad del backend S3 contra un servidor S3 **real** (HSP-139).
//!
//! La serie HSP-116 fijó el contrato contra LocalStack. Este fichero no
//! reescribe ese contrato: lo **aplica** contra RustFS, un servidor S3
//! real en Rust fijado por digest de imagen. La batería —AC1
//! concurrencia y reintento con la respuesta perdida, AC2 escritura
//! condicional honesta sobre un preexistente corrupto, AC3 fronteras
//! multipart y segado del staging huérfano, AC4 informe de capacidades
//! medido— vive en `common::conformity` y corre idéntica contra los dos
//! bancos: lo que se afirma del emulador tiene que ser cierto del
//! servidor, y los veredictos del informe llevan la nota de versión de
//! esta imagen.
//!
//! Lo que a propósito **no** se finge: >5 GiB no se promete en el
//! producto ni cabe en la mesa de esta máquina con holgura; el límite
//! que se prueba es el que el código declara y el que el desplegador
//! puede pedir. El informe de AC4 lo deja escrito.

mod common;

use hodei_hub::blob::BlobStore;

/// AC1: publicadores concurrentes del mismo digest convergen a un
/// objeto íntegro y a una sola referencia.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishers_of_one_digest_converge_to_one_intact_object() {
    let bucket = common::rustfs::a_unique_bucket("ac1-conc");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac1_concurrent_publishers(&client, &bucket).await;
}

/// AC1 (mitad dura): el reintento tras una respuesta perdida publica
/// el mismo objeto una sola vez.
#[tokio::test]
async fn a_retry_after_a_lost_ack_publishes_the_same_object_once() {
    let bucket = common::rustfs::a_unique_bucket("ac1-retry");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac1_lost_ack_retry(&client, &bucket).await;
}

/// AC2: un blob preexistente corrupto no se presenta como
/// deduplicación válida — se llama corrupto y se repara.
#[tokio::test]
async fn a_preexisting_corrupt_blob_is_never_presented_as_a_valid_dedupe() {
    let bucket = common::rustfs::a_unique_bucket("ac2-cond");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac2_corrupt_is_never_dedupe(&client, &bucket).await;
}

/// AC2 (mitad de sesión): el publish condicionado de la sesión
/// deduplica honestamente sobre un objeto bueno ya existente.
#[tokio::test]
async fn the_session_publish_dedupes_honestly_over_an_existing_object() {
    let bucket = common::rustfs::a_unique_bucket("ac2-session");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac2_session_dedupes(&client, &bucket).await;
}

/// AC3: multipart en la frontera del trozo mínimo y abort sin rastro.
#[tokio::test]
async fn multipart_at_the_frontier_and_abort_leaves_no_trace() {
    let bucket = common::rustfs::a_unique_bucket("ac3-multipart");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac3_multipart_and_abort(&client, &bucket).await;
}

/// AC3 (mitad de GC): el segado aborta los huérfanos propios, respeta
/// los vivos y no toca las claves ajenas.
#[tokio::test]
async fn the_staging_reaper_aborts_orphans_and_spares_foreign_keys() {
    let bucket = common::rustfs::a_unique_bucket("ac3-reaper");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac3_staging_reaper(&client, &bucket).await;
}

/// AC4: el informe de capacidades, medido contra esta versión de
/// RustFS. Cada línea sale de una llamada real al servidor de esta
/// ejecución; las aserciones fijan lo que el producto necesita.
#[tokio::test]
async fn the_capability_report_is_measured_not_assumed() {
    let bucket = common::rustfs::a_unique_bucket("ac4-report");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    for line in common::conformity::ac4_capability_report(&client, &bucket).await {
        println!("CAPACIDAD [{}] {line}", common::rustfs::VERSION_NOTE);
    }
}

/// La suite de paridad de HSP-116, aplicada sin retoques al servidor
/// real: el literal del enunciado ("aplicar la suite de contrato
/// existente ... a un servidor S3 real").
#[tokio::test]
async fn the_parity_suite_runs_against_the_real_server() {
    let bucket = common::rustfs::a_unique_bucket("parity");
    let store = common::rustfs::a_store(&bucket).await;
    common::the_suite(&store).await;
    // El restart, también: el estado vive en el servidor, no en el
    // cliente — un almacén nuevo sobre el mismo cubo reencuentra el
    // contenido publicado por el anterior.
    let bytes = common::the_bytes();
    let digest = store.put(&bytes).await.expect("publicado");
    drop(store);
    let reopened = common::rustfs::a_store(&bucket).await;
    reopened.verify(&digest).await.expect("revive en el real");
    assert_eq!(reopened.get(&digest).await.expect("y se lee"), bytes);
}
