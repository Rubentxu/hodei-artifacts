//! La misma conformidad de HSP-139, corrida contra el otro banco:
//! LocalStack 4.14, el servidor S3 del desarrollo (HSP-116).
//!
//! No es repetición: es la **matriz** que el AC4 pide — el informe de
//! capacidades por backend y versión, con los resultados medidos contra
//! cada uno. Donde RustFS es la palabra del servidor real, LocalStack
//! es la palabra del banco con el que se trabaja a diario; si algún día
//! difieren en una capacidad que el producto usa, este par de ficheros
//! es el sitio donde se ve.
//!
//! El arnés de LocalStack es el mismo de HSP-116; lo único propio de
//! aquí es que cada test pide su cubo y corre la batería compartida de
//! `common::conformity`.

mod common;

/// AC1: publicadores concurrentes del mismo digest convergen a un
/// objeto íntegro y a una sola referencia.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishers_of_one_digest_converge_to_one_intact_object() {
    let bucket = common::localstack::a_unique_bucket("ac1-conc");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac1_concurrent_publishers(&client, &bucket).await;
}

/// AC1 (mitad dura): el reintento tras una respuesta perdida publica
/// el mismo objeto una sola vez.
#[tokio::test]
async fn a_retry_after_a_lost_ack_publishes_the_same_object_once() {
    let bucket = common::localstack::a_unique_bucket("ac1-retry");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac1_lost_ack_retry(&client, &bucket).await;
}

/// AC2: un blob preexistente corrupto no se presenta como
/// deduplicación válida — se llama corrupto y se repara.
#[tokio::test]
async fn a_preexisting_corrupt_blob_is_never_presented_as_a_valid_dedupe() {
    let bucket = common::localstack::a_unique_bucket("ac2-cond");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac2_corrupt_is_never_dedupe(&client, &bucket).await;
}

/// AC2 (mitad de sesión): el publish condicionado de la sesión
/// deduplica honestamente sobre un objeto bueno ya existente.
#[tokio::test]
async fn the_session_publish_dedupes_honestly_over_an_existing_object() {
    let bucket = common::localstack::a_unique_bucket("ac2-session");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac2_session_dedupes(&client, &bucket).await;
}

/// AC3: multipart en la frontera del trozo mínimo y abort sin rastro.
#[tokio::test]
async fn multipart_at_the_frontier_and_abort_leaves_no_trace() {
    let bucket = common::localstack::a_unique_bucket("ac3-multipart");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac3_multipart_and_abort(&client, &bucket).await;
}

/// AC3 (mitad de GC): el segado aborta los huérfanos propios, respeta
/// los vivos y no toca las claves ajenas.
#[tokio::test]
async fn the_staging_reaper_aborts_orphans_and_spares_foreign_keys() {
    let bucket = common::localstack::a_unique_bucket("ac3-reaper");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac3_staging_reaper(&client, &bucket).await;
}

/// AC4: el informe de capacidades de LocalStack 4.14, medido contra
/// este banco. La otra mitad de la matriz está en el fichero hermanado
/// de RustFS.
#[tokio::test]
async fn the_capability_report_is_measured_not_assumed() {
    let bucket = common::localstack::a_unique_bucket("ac4-report");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    for line in common::conformity::ac4_capability_report(&client, &bucket).await {
        println!("CAPACIDAD [{}] {line}", common::localstack::VERSION_NOTE);
    }
}
